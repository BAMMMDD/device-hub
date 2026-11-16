//! DEVHUB: GPU backdrop filter for `Scene::backdrops` (Liquid Glass).
//!
//! Per batch of backdrops that share a draw order:
//! 1. read the render target directly (the swap chain is shader-readable),
//! 2. shrink the part the batch reads level by level (`backdrop_down`) until the blur's sigma is a
//!    few texels, then blur that level separably (`backdrop_blur`),
//! 3. composite every region from the blurred level (`backdrop`): rim
//!    refraction, saturate(), brightness(), rim light, rounded-corner coverage.
//!
//! Only the union of the batch's footprints is filtered, so a small button
//! costs a small rectangle. Unblurred regions (a refracting droplet) read a
//! copy of their own rectangle, since the target cannot be sampled while it
//! is being drawn to. Textures are created on first use and
//! dropped on resize / device loss, so windows without glass pay nothing.

use std::slice;

use anyhow::{Context, Result};
use windows::Win32::Graphics::{Direct3D::*, Direct3D11::*, Dxgi::Common::*};
use windows::core::Interface;

use super::{
    PipelineState, RENDER_TARGET_FORMAT, create_constant_buffer,
    create_fragment_shader, create_vertex_shader, update_batch_start, update_buffer,
};
use crate::directx_renderer::shader_resources::{RawShaderBytes, ShaderModule, ShaderTarget};
use gpui::{Backdrop, ScaledPixels};

/// Deepest level (1/32 resolution). sigma 64 device px still lands on ~2 texels.
const MAX_LEVEL: usize = 5;
/// Largest tap radius of the separable blur (texels each side).
const MAX_RADIUS: f32 = 16.0;
/// Allocation grain of the size-dependent textures.
const GRAIN: u32 = 256;

#[derive(Clone, Copy, Default)]
#[repr(C, align(16))]
struct PassParams {
    dst_rect: [f32; 4],
    dst_size: [f32; 2],
    src_size: [f32; 2],
    src_clamp: [f32; 4],
    dir: [f32; 2],
    sigma: f32,
    radius: f32,
    level_scale: f32,
    _pad: f32,
    /// Texel position of the source's (0, 0) in the pass's coordinates.
    src_origin: [f32; 2],
    /// Full-resolution snapshot for the clear refracting edge, independent of frost.
    sharp_size: [f32; 2],
    sharp_origin: [f32; 2],
    sharp_clamp: [f32; 4],
}

const _: () = assert!(std::mem::size_of::<PassParams>() == 112);

struct Target {
    texture: ID3D11Texture2D,
    rtv: Option<ID3D11RenderTargetView>,
    srv: Option<ID3D11ShaderResourceView>,
    width: u32,
    height: u32,
}

impl Target {
    fn new(device: &ID3D11Device, width: u32, height: u32, render_target: bool) -> Result<Self> {
        let desc = D3D11_TEXTURE2D_DESC {
            Width: width.max(1),
            Height: height.max(1),
            MipLevels: 1,
            ArraySize: 1,
            Format: RENDER_TARGET_FORMAT,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: if render_target {
                (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32
            } else {
                D3D11_BIND_SHADER_RESOURCE.0 as u32
            },
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        let mut texture = None;
        unsafe { device.CreateTexture2D(&desc, None, Some(&mut texture))? };
        let texture = texture.context("backdrop texture")?;
        let mut srv = None;
        unsafe { device.CreateShaderResourceView(&texture, None, Some(&mut srv))? };
        let mut rtv = None;
        if render_target {
            unsafe { device.CreateRenderTargetView(&texture, None, Some(&mut rtv))? };
        }
        Ok(Self { texture, rtv, srv, width: width.max(1), height: height.max(1) })
    }
}

/// Size-dependent textures, created lazily.
struct Textures {
    width: u32,
    height: u32,
    capture: Option<Target>,
    /// `levels[i]` is 1/2^(i+1) resolution, `scratch[i]` its blur ping-pong partner.
    levels: Vec<Option<Target>>,
    scratch: Vec<Option<Target>>,
}

impl Textures {
    fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            capture: None,
            levels: (0..MAX_LEVEL).map(|_| None).collect(),
            scratch: (0..MAX_LEVEL).map(|_| None).collect(),
        }
    }

    fn level_size(&self, level: usize) -> (u32, u32) {
        (self.width.div_ceil(1 << level).max(1), self.height.div_ceil(1 << level).max(1))
    }
}

pub(crate) struct BackdropPipeline {
    down_vs: ID3D11VertexShader,
    down_ps: ID3D11PixelShader,
    blur_vs: ID3D11VertexShader,
    blur_ps: ID3D11PixelShader,
    composite: PipelineState<Backdrop>,
    opaque: ID3D11BlendState,
    params: Option<ID3D11Buffer>,
    sampler: Option<ID3D11SamplerState>,
    textures: Option<Textures>,
    /// (raw pointer of the render target it was made for, view).
    rt_srv: Option<(usize, Option<ID3D11ShaderResourceView>)>,
}

/// A rectangle in device pixels (x0, y0, x1, y1), integer aligned.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Rect {
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
}

impl Rect {
    fn empty() -> Self {
        Rect { x0: i32::MAX, y0: i32::MAX, x1: i32::MIN, y1: i32::MIN }
    }
    fn is_empty(&self) -> bool {
        self.x0 >= self.x1 || self.y0 >= self.y1
    }
    fn union(self, o: Rect) -> Rect {
        Rect { x0: self.x0.min(o.x0), y0: self.y0.min(o.y0), x1: self.x1.max(o.x1), y1: self.y1.max(o.y1) }
    }
    fn clamp_to(self, w: u32, h: u32) -> Rect {
        Rect { x0: self.x0.max(0), y0: self.y0.max(0), x1: self.x1.min(w as i32), y1: self.y1.min(h as i32) }
    }
    /// Grow outwards to a multiple of `grid` so every level's texels stay
    /// anchored to the screen (static content does not swim while another
    /// region of the batch moves).
    fn align(self, grid: i32) -> Rect {
        Rect {
            x0: self.x0.div_euclid(grid) * grid,
            y0: self.y0.div_euclid(grid) * grid,
            x1: (self.x1 + grid - 1).div_euclid(grid) * grid,
            y1: (self.y1 + grid - 1).div_euclid(grid) * grid,
        }
    }
    /// The same area in level `level` texels (rounded outwards).
    fn at_level(self, level: usize) -> Rect {
        let s = 1 << level;
        Rect { x0: self.x0.div_euclid(s), y0: self.y0.div_euclid(s), x1: (self.x1 + s - 1).div_euclid(s), y1: (self.y1 + s - 1).div_euclid(s) }
    }
    fn clamp_params(self) -> [f32; 4] {
        [self.x0 as f32 + 0.5, self.y0 as f32 + 0.5, self.x1 as f32 - 0.5, self.y1 as f32 - 0.5]
    }
    fn dst(self) -> [f32; 4] {
        [self.x0 as f32, self.y0 as f32, (self.x1 - self.x0) as f32, (self.y1 - self.y0) as f32]
    }
}

/// Which level a blur of `sigma` device pixels runs at, and the sigma there.
fn level_for(sigma: f32) -> (usize, f32) {
    if sigma < 0.5 {
        return (0, 0.0);
    }
    let level = ((sigma / 2.0).log2().floor() as i32).clamp(1, MAX_LEVEL as i32) as usize;
    (level, sigma / (1 << level) as f32)
}

/// Pixels a backdrop reads, in device pixels: its visible part grown by the
/// blur kernel. Refraction bends inwards, so it never reads outside.
fn footprint(b: &Backdrop) -> Rect {
    let visible = b.bounds.intersect(&b.content_mask.bounds);
    let grow = b.blur * 3.0 + 2.0;
    let ScaledPixels(x) = visible.origin.x;
    let ScaledPixels(y) = visible.origin.y;
    let ScaledPixels(w) = visible.size.width;
    let ScaledPixels(h) = visible.size.height;
    Rect {
        x0: (x - grow).floor() as i32,
        y0: (y - grow).floor() as i32,
        x1: (x + w + grow).ceil() as i32,
        y1: (y + h + grow).ceil() as i32,
    }
}

impl BackdropPipeline {
    pub(crate) fn new(device: &ID3D11Device, composite_blend: ID3D11BlendState) -> Result<Self> {
        let vs = |module| -> Result<ID3D11VertexShader> {
            let raw = RawShaderBytes::new(module, ShaderTarget::Vertex)?;
            create_vertex_shader(device, raw.as_bytes())
        };
        let ps = |module| -> Result<ID3D11PixelShader> {
            let raw = RawShaderBytes::new(module, ShaderTarget::Fragment)?;
            create_fragment_shader(device, raw.as_bytes())
        };
        let opaque = unsafe {
            let mut desc = D3D11_BLEND_DESC::default();
            desc.RenderTarget[0].BlendEnable = false.into();
            desc.RenderTarget[0].RenderTargetWriteMask = D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8;
            let mut state = None;
            device.CreateBlendState(&desc, Some(&mut state))?;
            state.context("backdrop blend state")?
        };
        let sampler = unsafe {
            let desc = D3D11_SAMPLER_DESC {
                Filter: D3D11_FILTER_MIN_MAG_MIP_LINEAR,
                AddressU: D3D11_TEXTURE_ADDRESS_CLAMP,
                AddressV: D3D11_TEXTURE_ADDRESS_CLAMP,
                AddressW: D3D11_TEXTURE_ADDRESS_CLAMP,
                MipLODBias: 0.0,
                MaxAnisotropy: 1,
                ComparisonFunc: D3D11_COMPARISON_ALWAYS,
                BorderColor: [0.0; 4],
                MinLOD: 0.0,
                MaxLOD: D3D11_FLOAT32_MAX,
            };
            let mut output = None;
            device.CreateSamplerState(&desc, Some(&mut output))?;
            output
        };
        Ok(Self {
            down_vs: vs(ShaderModule::BackdropDown)?,
            down_ps: ps(ShaderModule::BackdropDown)?,
            blur_vs: vs(ShaderModule::BackdropBlur)?,
            blur_ps: ps(ShaderModule::BackdropBlur)?,
            composite: PipelineState::new(device, "backdrop_pipeline", ShaderModule::Backdrop, 16, composite_blend)?,
            opaque,
            params: create_constant_buffer::<PassParams>(device)?,
            sampler,
            textures: None,
            rt_srv: None,
        })
    }

    /// The swap chain was resized: its buffers (and the view on them) are new.
    /// The blur textures are kept while they still fit (`ensure_textures`).
    pub(crate) fn reset_textures(&mut self) {
        self.rt_srv = None;
    }

    pub(crate) fn upload(&mut self, device: &ID3D11Device, context: &ID3D11DeviceContext, backdrops: &[Backdrop]) -> Result<()> {
        if backdrops.is_empty() {
            return Ok(());
        }
        self.composite.update_buffer(device, context, backdrops)
    }

    /// Textures are sized to a 256px grain with slack, so dragging the window
    /// edge does not reallocate every frame; they shrink once the window is
    /// much smaller. Passes address texels absolutely, so spare size is unused.
    fn ensure_textures(&mut self, width: u32, height: u32) {
        let fits = |t: &Textures| {
            t.width >= width && t.height >= height && t.width <= width + width / 2 + GRAIN && t.height <= height + height / 2 + GRAIN
        };
        if !self.textures.as_ref().is_some_and(fits) {
            self.textures = Some(Textures::new(width.div_ceil(GRAIN) * GRAIN, height.div_ceil(GRAIN) * GRAIN));
        }
    }

    fn params(&self, context: &ID3D11DeviceContext, p: PassParams) -> Result<()> {
        update_buffer(context, self.params.as_ref().context("backdrop params")?, &[p])?;
        unsafe {
            context.VSSetConstantBuffers(2, Some(slice::from_ref(&self.params)));
            context.PSSetConstantBuffers(2, Some(slice::from_ref(&self.params)));
        }
        Ok(())
    }

    /// One full-screen-free pass: draw `dst` of `target` reading `src`.
    #[allow(clippy::too_many_arguments)]
    fn pass(
        &self,
        context: &ID3D11DeviceContext,
        vs: &ID3D11VertexShader,
        ps: &ID3D11PixelShader,
        target: &Target,
        src: &Option<ID3D11ShaderResourceView>,
        p: PassParams,
    ) -> Result<()> {
        self.params(context, p)?;
        unsafe {
            context.PSSetShaderResources(0, Some(&[None]));
            context.OMSetRenderTargets(Some(slice::from_ref(&target.rtv)), None);
            context.RSSetViewports(Some(&[D3D11_VIEWPORT {
                TopLeftX: 0.0,
                TopLeftY: 0.0,
                Width: target.width as f32,
                Height: target.height as f32,
                MinDepth: 0.0,
                MaxDepth: 1.0,
            }]));
            context.PSSetShaderResources(0, Some(slice::from_ref(src)));
            context.PSSetSamplers(1, Some(slice::from_ref(&self.sampler)));
            context.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP);
            context.VSSetShader(vs, None);
            context.PSSetShader(ps, None);
            context.OMSetBlendState(&self.opaque, None, 0xFFFFFFFF);
            context.Draw(4, 0);
        }
        Ok(())
    }

    /// The render target as a shader input (the swap chain is created with
    /// `DXGI_USAGE_SHADER_INPUT`), so the first downsample reads it directly
    /// instead of copying the window. `None` if the driver refuses; then every
    /// batch copies its region first.
    fn target_srv(&mut self, device: &ID3D11Device, render_target: &ID3D11Texture2D) -> Option<ID3D11ShaderResourceView> {
        let raw = render_target.as_raw() as usize;
        if let Some((key, srv)) = &self.rt_srv {
            if *key == raw {
                return srv.clone();
            }
        }
        let mut srv = None;
        let ok = unsafe { device.CreateShaderResourceView(render_target, None, Some(&mut srv)) }.is_ok();
        let srv = if ok { srv } else { None };
        self.rt_srv = Some((raw, srv.clone()));
        srv
    }

    /// Copy `region` of the render target into a texture of its own size.
    /// Returns the source (with `region`'s origin, so device coordinates still map).
    fn copy_region(&mut self, device: &ID3D11Device, context: &ID3D11DeviceContext, render_target: &ID3D11Texture2D, region: Rect) -> Result<Source> {
        let (w, h) = ((region.x1 - region.x0) as u32, (region.y1 - region.y0) as u32);
        let textures = self.textures.as_mut().unwrap();
        // Grow-only, so a moving region does not reallocate every frame.
        if textures.capture.as_ref().is_none_or(|c| c.width < w || c.height < h) {
            let (cw, ch) = textures.capture.as_ref().map(|c| (c.width.max(w), c.height.max(h))).unwrap_or((w, h));
            textures.capture = Some(Target::new(device, cw, ch, false)?);
        }
        let capture = textures.capture.as_ref().unwrap();
        let src_box = D3D11_BOX { left: region.x0 as u32, top: region.y0 as u32, front: 0, right: region.x1 as u32, bottom: region.y1 as u32, back: 1 };
        unsafe {
            context.OMSetRenderTargets(None, None);
            context.PSSetShaderResources(0, Some(&[None]));
            context.PSSetShaderResources(2, Some(&[None]));
            context.CopySubresourceRegion(&capture.texture, 0, 0, 0, 0, render_target, 0, Some(&src_box));
        }
        Ok(Source { srv: capture.srv.clone(), width: capture.width, height: capture.height, origin: [region.x0 as f32, region.y0 as f32] })
    }

    /// Filter and composite `backdrops` (instances `start..start + len` of the
    /// uploaded buffer) onto `render_target`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn draw(
        &mut self,
        device: &ID3D11Device,
        context: &ID3D11DeviceContext,
        render_target: &ID3D11Texture2D,
        render_target_view: &Option<ID3D11RenderTargetView>,
        viewport: &D3D11_VIEWPORT,
        batch_params: &ID3D11Buffer,
        backdrops: &[Backdrop],
        start: usize,
    ) -> Result<()> {
        let (width, height) = (viewport.Width as u32, viewport.Height as u32);
        if backdrops.is_empty() || width == 0 || height == 0 {
            return Ok(());
        }
        self.ensure_textures(width, height);
        unsafe { context.OMSetRenderTargets(None, None) };
        let result = self.draw_groups(device, context, render_target, render_target_view, viewport, batch_params, backdrops, start, width, height);

        // Leave the pipeline as the other primitives expect it.
        unsafe {
            context.PSSetShaderResources(0, Some(&[None]));
            context.PSSetShaderResources(2, Some(&[None]));
            context.OMSetRenderTargets(Some(slice::from_ref(render_target_view)), None);
            context.RSSetViewports(Some(slice::from_ref(viewport)));
        }
        result
    }

    /// Blur and composite, one group per blur level (most batches have one).
    #[allow(clippy::too_many_arguments)]
    fn draw_groups(
        &mut self,
        device: &ID3D11Device,
        context: &ID3D11DeviceContext,
        render_target: &ID3D11Texture2D,
        render_target_view: &Option<ID3D11RenderTargetView>,
        viewport: &D3D11_VIEWPORT,
        batch_params: &ID3D11Buffer,
        backdrops: &[Backdrop],
        start: usize,
        width: u32,
        height: u32,
    ) -> Result<()> {
        let key_of = |b: &Backdrop| {
            let (level, sigma) = level_for(b.blur);
            (level, (sigma * 8.0).round() as i32)
        };
        let mut done = vec![false; backdrops.len()];
        for first in 0..backdrops.len() {
            if done[first] {
                continue;
            }
            let key = key_of(&backdrops[first]);
            let (level, sigma) = level_for(backdrops[first].blur);
            let members: Vec<usize> = (first..backdrops.len())
                .filter(|&i| !done[i] && key_of(&backdrops[i]) == key)
                .collect();
            for &i in &members {
                done[i] = true;
            }
            // The group's own region keeps a small button from blurring the
            // whole batch at a deep level.
            let group_region = members
                .iter()
                .map(|&i| footprint(&backdrops[i]))
                .fold(Rect::empty(), Rect::union)
                .align(1 << MAX_LEVEL)
                .clamp_to(width, height);
            if group_region.is_empty() {
                continue;
            }
            // Where the composite reads from: a blurred level, or (no blur) a
            // copy of the region, since the target cannot be read while drawn to.
            // Keep an immutable, sharp copy before filtering. Sampling the swap
            // chain while compositing into it is a D3D11 resource hazard.
            let sharp = if members.iter().any(|&i| backdrops[i].bezel > 0.5 || backdrops[i].glass[0] > 0.5) {
                Some(self.copy_region(device, context, render_target, group_region)?)
            } else {
                None
            };
            let source = if let Some(ref sharp) = sharp {
                if level == 0 {
                    Source { srv: sharp.srv.clone(), width: sharp.width, height: sharp.height, origin: sharp.origin }
                } else {
                    self.filter(device, context, sharp, group_region, level, sigma)?
                }
            } else if level == 0 {
                self.copy_region(device, context, render_target, group_region)?
            } else {
                let first_source = match self.target_srv(device, render_target) {
                    Some(srv) => Source { srv: Some(srv), width, height, origin: [0.0, 0.0] },
                    None => self.copy_region(device, context, render_target, group_region)?,
                };
                self.filter(device, context, &first_source, group_region, level, sigma)?
            };
            let sharp = sharp.as_ref().unwrap_or(&source);
            let clamp = group_region.at_level(level).clamp_params();
            self.params(
                context,
                PassParams {
                    src_size: [source.width as f32, source.height as f32],
                    src_clamp: clamp,
                    level_scale: (1u32 << level) as f32,
                    src_origin: source.origin,
                    sharp_size: [sharp.width as f32, sharp.height as f32],
                    sharp_origin: sharp.origin,
                    sharp_clamp: group_region.clamp_params(),
                    ..Default::default()
                },
            )?;
            unsafe {
                context.PSSetShaderResources(0, Some(&[None]));
                context.OMSetRenderTargets(Some(slice::from_ref(render_target_view)), None);
                context.RSSetViewports(Some(slice::from_ref(viewport)));
                context.PSSetShaderResources(0, Some(slice::from_ref(&source.srv)));
                context.PSSetShaderResources(2, Some(slice::from_ref(&sharp.srv)));
                context.PSSetSamplers(1, Some(slice::from_ref(&self.sampler)));
            }
            for &i in &members {
                update_batch_start(context, batch_params, (start + i) as u32)?;
                self.composite.draw(context, D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP, 4, 1)?;
            }
            unsafe {
                context.PSSetShaderResources(0, Some(&[None]));
                context.PSSetShaderResources(2, Some(&[None]));
                context.OMSetRenderTargets(None, None);
            }
        }
        Ok(())
    }

    /// Downsample `first` (full resolution) over `region` to `level` and blur
    /// it there. Returns the blurred level as the composite's source.
    fn filter(&mut self, device: &ID3D11Device, context: &ID3D11DeviceContext, first: &Source, region: Rect, level: usize, sigma: f32) -> Result<Source> {
        // Create the levels this blur needs.
        {
            let textures = self.textures.as_mut().unwrap();
            for i in 1..=level {
                let (w, h) = textures.level_size(i);
                if textures.levels[i - 1].is_none() {
                    textures.levels[i - 1] = Some(Target::new(device, w, h, true)?);
                }
            }
            if textures.scratch[level - 1].is_none() {
                let (w, h) = textures.level_size(level);
                textures.scratch[level - 1] = Some(Target::new(device, w, h, true)?);
            }
        }
        let textures = self.textures.as_ref().unwrap();
        // Downsample chain.
        for i in 1..=level {
            let dst = textures.levels[i - 1].as_ref().unwrap();
            let (src_srv, src_w, src_h, origin) = if i == 1 {
                (&first.srv, first.width, first.height, first.origin)
            } else {
                let t = textures.levels[i - 2].as_ref().unwrap();
                (&t.srv, t.width, t.height, [0.0, 0.0])
            };
            self.pass(
                context,
                &self.down_vs,
                &self.down_ps,
                dst,
                src_srv,
                PassParams {
                    dst_rect: region.at_level(i).dst(),
                    dst_size: [dst.width as f32, dst.height as f32],
                    src_size: [src_w as f32, src_h as f32],
                    src_clamp: region.at_level(i - 1).clamp_params(),
                    src_origin: origin,
                    ..Default::default()
                },
            )?;
        }
        // Separable gaussian at the last level, horizontal into scratch, vertical back.
        // The downsample already blurred by roughly one texel; take that off.
        let sigma = (sigma * sigma - 0.6).max(0.25).sqrt();
        let radius = (sigma * 3.0).ceil().min(MAX_RADIUS);
        let lvl = textures.levels[level - 1].as_ref().unwrap();
        let scratch = textures.scratch[level - 1].as_ref().unwrap();
        let r = region.at_level(level);
        let base = PassParams {
            dst_rect: r.dst(),
            dst_size: [lvl.width as f32, lvl.height as f32],
            src_size: [lvl.width as f32, lvl.height as f32],
            src_clamp: r.clamp_params(),
            sigma,
            radius,
            ..Default::default()
        };
        self.pass(context, &self.blur_vs, &self.blur_ps, scratch, &lvl.srv, PassParams { dir: [1.0, 0.0], ..base })?;
        self.pass(context, &self.blur_vs, &self.blur_ps, lvl, &scratch.srv, PassParams { dir: [0.0, 1.0], ..base })?;
        Ok(Source { srv: lvl.srv.clone(), width: lvl.width, height: lvl.height, origin: [0.0, 0.0] })
    }
}

/// A texture the next pass samples, and where its texel (0, 0) sits in that
/// pass's coordinates (non-zero only for a region-sized copy).
struct Source {
    srv: Option<ID3D11ShaderResourceView>,
    width: u32,
    height: u32,
    origin: [f32; 2],
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sharp_snapshot_constants_match_hlsl_register_packing() {
        // b2 packs the original parameters into five 16-byte registers,
        // then (size, origin) and clamp into two more. A mismatch silently
        // samples unrelated pixels, especially with region-sized captures.
        assert_eq!(std::mem::offset_of!(PassParams, sharp_size), 80);
        assert_eq!(std::mem::offset_of!(PassParams, sharp_origin), 88);
        assert_eq!(std::mem::offset_of!(PassParams, sharp_clamp), 96);
        assert_eq!(std::mem::size_of::<PassParams>(), 112);
        assert_eq!(std::mem::offset_of!(Backdrop, glass), 132);
        assert_eq!(std::mem::size_of::<Backdrop>(), 148);
    }

    #[test]
    fn levels_keep_sigma_small_and_skip_tiny_blurs() {
        assert_eq!(level_for(0.0), (0, 0.0));
        let (l, s) = level_for(30.0);
        assert_eq!(l, 3);
        assert!((s - 3.75).abs() < 1e-4);
        let (l, s) = level_for(3.0);
        assert_eq!(l, 1);
        assert!((s - 1.5).abs() < 1e-4);
        assert_eq!(level_for(1000.0).0, MAX_LEVEL);
    }

    #[test]
    fn regions_align_outwards_and_clamp() {
        let r = Rect { x0: 33, y0: -5, x1: 70, y1: 64 }.align(32).clamp_to(100, 50);
        assert_eq!(r, Rect { x0: 32, y0: 0, x1: 96, y1: 50 });
        assert_eq!(Rect { x0: 32, y0: 0, x1: 96, y1: 50 }.at_level(3), Rect { x0: 4, y0: 0, x1: 12, y1: 7 });
    }
}
