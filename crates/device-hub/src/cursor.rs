//! 計測中にカーソルをアプリ窓内に固定する (ゲームと同じ ClipCursor)。
//!
//! ポーリング実測 (docs/POLLING_RATE.md) で使う — 5 秒計測の間だけ
//! カーソルをウィンドウ内に閉じ込めると、自然に窓内で速い円を描く動きになり、
//! アプリが背面に行って raw input が制限される事故も防げる。
//! 解除は必ず対で呼ぶこと (プロセス終了時は OS が自動解除するが、
//! 計測中に窓が閉じても tick 側で解除する)。

#[cfg(windows)]
pub fn clip_to_window(window: &gpui_kit::Window) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows_sys::Win32::Foundation::{HWND, RECT};
    use windows_sys::Win32::UI::WindowsAndMessaging::{ClipCursor, GetWindowRect};

    let Ok(h) = HasWindowHandle::window_handle(window) else {
        return;
    };
    let RawWindowHandle::Win32(w) = h.as_raw() else {
        return;
    };
    unsafe {
        let hwnd = w.hwnd.get() as HWND;
        let mut r = RECT::default();
        if GetWindowRect(hwnd, &mut r) != 0 {
            ClipCursor(&r);
        }
    }
}

#[cfg(windows)]
pub fn unclip() {
    unsafe { windows_sys::Win32::UI::WindowsAndMessaging::ClipCursor(std::ptr::null()) };
}

#[cfg(not(windows))]
pub fn clip_to_window(_window: &gpui_kit::Window) {}
#[cfg(not(windows))]
pub fn unclip() {}
