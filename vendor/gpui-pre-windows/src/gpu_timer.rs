//! DEVHUB: GPU timestamp queries for measuring what a frame costs on the GPU
//! (`DEVHUB_GPUI_GPUTIME=1`). Off by default: no queries are created.
//!
//! Each frame records begin/end timestamps, plus a pair around every backdrop
//! batch. Results are read a few frames later without stalling, and a summary
//! line goes to stderr every 240 frames:
//! `GPUTIME frames=240 frame p50=.. p95=.. max=.. backdrop p50=.. p95=.. batches/frame=..`

use std::collections::VecDeque;

use windows::Win32::Graphics::Direct3D11::*;
use windows::core::Interface;

const MAX_BATCHES: usize = 32;
const REPORT_EVERY: usize = 240;

struct FrameQueries {
    disjoint: ID3D11Query,
    begin: ID3D11Query,
    end: ID3D11Query,
    batches: Vec<(ID3D11Query, ID3D11Query)>,
    used: usize,
}

pub(crate) struct GpuTimer {
    in_flight: VecDeque<FrameQueries>,
    pool: Vec<FrameQueries>,
    current: Option<FrameQueries>,
    frame_ms: Vec<f32>,
    backdrop_ms: Vec<f32>,
    batches: usize,
}

fn query(device: &ID3D11Device, kind: D3D11_QUERY) -> Option<ID3D11Query> {
    let desc = D3D11_QUERY_DESC { Query: kind, MiscFlags: 0 };
    let mut q = None;
    unsafe { device.CreateQuery(&desc, Some(&mut q)).ok()? };
    q
}

/// `GetData` without flushing; `None` while the GPU has not finished.
fn read<T: Default>(context: &ID3D11DeviceContext, q: &ID3D11Query) -> Option<T> {
    let mut out = T::default();
    let hr = unsafe {
        (Interface::vtable(context).GetData)(
            Interface::as_raw(context),
            Interface::as_raw(q),
            &mut out as *mut T as *mut _,
            std::mem::size_of::<T>() as u32,
            D3D11_ASYNC_GETDATA_DONOTFLUSH.0 as u32,
        )
    };
    (hr.0 == 0).then_some(out)
}

impl GpuTimer {
    pub(crate) fn from_env() -> Option<Self> {
        std::env::var_os("DEVHUB_GPUI_GPUTIME")?;
        Some(Self { in_flight: VecDeque::new(), pool: Vec::new(), current: None, frame_ms: Vec::new(), backdrop_ms: Vec::new(), batches: 0 })
    }

    pub(crate) fn begin_frame(&mut self, device: &ID3D11Device, context: &ID3D11DeviceContext) {
        let frame = self.pool.pop().or_else(|| {
            Some(FrameQueries {
                disjoint: query(device, D3D11_QUERY_TIMESTAMP_DISJOINT)?,
                begin: query(device, D3D11_QUERY_TIMESTAMP)?,
                end: query(device, D3D11_QUERY_TIMESTAMP)?,
                batches: Vec::new(),
                used: 0,
            })
        });
        let Some(mut frame) = frame else { return };
        frame.used = 0;
        unsafe {
            context.Begin(&frame.disjoint);
            context.End(&frame.begin);
        }
        self.current = Some(frame);
    }

    pub(crate) fn batch(&mut self, device: &ID3D11Device, context: &ID3D11DeviceContext, start: bool) {
        let Some(frame) = self.current.as_mut() else { return };
        if start {
            if frame.used >= MAX_BATCHES {
                return;
            }
            if frame.batches.len() <= frame.used {
                let (Some(a), Some(b)) = (query(device, D3D11_QUERY_TIMESTAMP), query(device, D3D11_QUERY_TIMESTAMP)) else { return };
                frame.batches.push((a, b));
            }
            unsafe { context.End(&frame.batches[frame.used].0) };
        } else if frame.used < MAX_BATCHES && frame.used < frame.batches.len() {
            unsafe { context.End(&frame.batches[frame.used].1) };
            frame.used += 1;
        }
    }

    pub(crate) fn end_frame(&mut self, context: &ID3D11DeviceContext) {
        if let Some(frame) = self.current.take() {
            unsafe {
                context.End(&frame.end);
                context.End(&frame.disjoint);
            }
            self.in_flight.push_back(frame);
        }
        // Collect every finished frame (oldest first) without waiting.
        while let Some(frame) = self.in_flight.front() {
            let Some(dj) = read::<D3D11_QUERY_DATA_TIMESTAMP_DISJOINT>(context, &frame.disjoint) else {
                if self.in_flight.len() > 8 {
                    // Too far behind (device lost or never signalled): drop it.
                    let f = self.in_flight.pop_front().unwrap();
                    self.pool.push(f);
                    continue;
                }
                break;
            };
            let frame = self.in_flight.pop_front().unwrap();
            if !dj.Disjoint.as_bool() && dj.Frequency > 0 {
                let ms = |a: u64, b: u64| (b.saturating_sub(a)) as f64 * 1000.0 / dj.Frequency as f64;
                if let (Some(a), Some(b)) = (read::<u64>(context, &frame.begin), read::<u64>(context, &frame.end)) {
                    self.frame_ms.push(ms(a, b) as f32);
                    let mut bd = 0.0;
                    for (s, e) in frame.batches.iter().take(frame.used) {
                        if let (Some(s), Some(e)) = (read::<u64>(context, s), read::<u64>(context, e)) {
                            bd += ms(s, e);
                        }
                    }
                    self.backdrop_ms.push(bd as f32);
                    self.batches += frame.used;
                }
            }
            self.pool.push(frame);
        }
        if self.frame_ms.len() >= REPORT_EVERY {
            let stats = |v: &mut Vec<f32>| {
                v.sort_by(|a, b| a.total_cmp(b));
                let at = |p: f32| v[((v.len() - 1) as f32 * p) as usize];
                (at(0.5), at(0.95), *v.last().unwrap())
            };
            let n = self.frame_ms.len();
            let (f50, f95, fmax) = stats(&mut self.frame_ms);
            let (b50, b95, _) = stats(&mut self.backdrop_ms);
            eprintln!(
                "GPUTIME frames={n} frame p50={f50:.3}ms p95={f95:.3}ms max={fmax:.3}ms backdrop p50={b50:.3}ms p95={b95:.3}ms batches/frame={:.1}",
                self.batches as f32 / n as f32
            );
            self.frame_ms.clear();
            self.backdrop_ms.clear();
            self.batches = 0;
        }
    }
}
