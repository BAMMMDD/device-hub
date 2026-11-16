//! 実時間アニメーションと要素の位置の記録。
//!
//! - `spring(key, target)` … 臨界減衰ばねで target へ寄せた今の値を返す。
//!   動いている間は `end_frame` が次フレームを要求する (止まれば描画も止まる =
//!   アイドル時 CPU/GPU 0 — ゲーム中に裏で開いていても軽い)。
//! - `track(id)` … 要素の矩形を毎フレーム記録する透明な canvas。
//!   スライダーのドラッグ・ポップアップの位置合わせ・ホバー判定に使う。

use gpui_kit::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::time::Instant;

#[derive(Clone, Copy)]
struct Spring {
    value: f32,
    vel: f32,
    frame: u64,
}

#[derive(Default)]
struct Sys {
    springs: HashMap<String, Spring>,
    rects: HashMap<String, [f32; 4]>,
    pointer: [f32; 2],
    down: bool,
    last: Option<Instant>,
    dt: f32,
    frame: u64,
    dirty: bool,
}

thread_local! {
    static SYS: RefCell<Sys> = RefCell::new(Sys { dt: 1. / 60., ..Default::default() });
}

/// フレームの先頭で呼ぶ。経過時間を測る。
pub fn begin_frame() {
    SYS.with(|s| {
        let mut s = s.borrow_mut();
        let now = Instant::now();
        s.dt = s
            .last
            .map(|l| (now - l).as_secs_f32())
            .unwrap_or(1. / 60.)
            .clamp(0.001, 0.05);
        s.last = Some(now);
        s.frame += 1;
        s.dirty = false;
        if s.springs.len() > 2048 {
            let f = s.frame;
            s.springs.retain(|_, v| f - v.frame < 600);
        }
    });
}

/// フレームの終わりで呼ぶ。ばねが動いていれば次のフレームを要求する。
pub fn end_frame(window: &mut Window) {
    let dirty = SYS.with(|s| {
        let mut s = s.borrow_mut();
        let d = s.dirty;
        if !d {
            s.last = None; // 止まっている間の時間は数えない
        }
        d
    });
    if dirty {
        window.request_animation_frame();
    }
}

/// 臨界減衰ばね。`speed` は大きいほど速い (12 ≒ 0.35s)。
pub fn spring_with(key: &str, target: f32, speed: f32) -> f32 {
    if !crate::theme::motion_on() {
        return target;
    }
    SYS.with(|s| {
        let mut s = s.borrow_mut();
        let dt = s.dt;
        let frame = s.frame;
        let sp = s.springs.entry(key.to_string()).or_insert(Spring {
            value: target,
            vel: 0.,
            frame,
        });
        if sp.frame == frame {
            return sp.value;
        }
        sp.frame = frame;
        // 臨界減衰: x'' = -w^2 (x - t) - 2w x'
        let w = speed;
        let mut x = sp.value;
        let mut v = sp.vel;
        // 半陰的オイラーを細かく刻んで安定させる。
        let steps = 4;
        let h = dt / steps as f32;
        for _ in 0..steps {
            let a = -w * w * (x - target) - 2. * w * v;
            v += a * h;
            x += v * h;
        }
        let settled = (x - target).abs() < 0.0008 && v.abs() < 0.002;
        if settled {
            x = target;
            v = 0.;
        }
        sp.value = x;
        sp.vel = v;
        if !settled {
            s.dirty = true;
        }
        x
    })
}

pub fn spring(key: &str, target: f32) -> f32 {
    spring_with(key, target, 16.)
}

/// 0/1 の状態をなめらかにした値。
pub fn ease(key: &str, on: bool) -> f32 {
    spring_with(key, if on { 1. } else { 0. }, 18.)
}

/// 外部から「次のフレームも描け」と伝える (時間で変わる表示用)。
pub fn keep_alive() {
    SYS.with(|s| s.borrow_mut().dirty = true);
}

// ------------------------------------------------------------------ 位置

/// 要素の矩形を記録する透明な層 (親いっぱいに広がる)。
pub fn track(id: impl Into<String>) -> impl IntoElement {
    let id: String = id.into();
    canvas(
        move |bounds, _, _| {
            SYS.with(|s| {
                s.borrow_mut().rects.insert(
                    id.clone(),
                    [
                        f32::from(bounds.origin.x),
                        f32::from(bounds.origin.y),
                        f32::from(bounds.size.width),
                        f32::from(bounds.size.height),
                    ],
                );
            });
        },
        |_, _, _, _| {},
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

/// 前のフレームで記録した矩形 [x, y, w, h]。
pub fn rect(id: &str) -> Option<[f32; 4]> {
    SYS.with(|s| s.borrow().rects.get(id).copied())
}

pub fn set_pointer(x: f32, y: f32) {
    SYS.with(|s| s.borrow_mut().pointer = [x, y]);
}

pub fn pointer() -> [f32; 2] {
    SYS.with(|s| s.borrow().pointer)
}

pub fn set_down(d: bool) {
    SYS.with(|s| s.borrow_mut().down = d);
}

pub fn is_down() -> bool {
    SYS.with(|s| s.borrow().down)
}

/// カーソルがその要素の上にあるか (前フレームの矩形で判定)。
pub fn hovered(id: &str) -> bool {
    let p = pointer();
    rect(id)
        .is_some_and(|r| p[0] >= r[0] && p[0] <= r[0] + r[2] && p[1] >= r[1] && p[1] <= r[1] + r[3])
}
