//! 単一インスタンス制御とシステムトレイ常駐。

#[cfg(windows)]
mod win {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
    use windows_sys::Win32::Foundation::{
        GetLastError, ERROR_ALREADY_EXISTS, HWND, LPARAM, LRESULT, POINT, WPARAM,
    };
    use windows_sys::Win32::Graphics::Gdi::{CreateBitmap, DeleteObject, HBITMAP};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::System::Threading::CreateMutexW;
    use windows_sys::Win32::UI::Shell::{
        Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        AppendMenuW, CreateIconIndirect, CreatePopupMenu, CreateWindowExW, DefWindowProcW,
        DestroyIcon, DestroyMenu, DispatchMessageW, FindWindowW, GetCursorPos, GetMessageW,
        PostMessageW, PostQuitMessage, RegisterClassW, SetForegroundWindow, ShowWindow,
        TrackPopupMenu, TranslateMessage, HICON, ICONINFO, MF_CHECKED, MF_SEPARATOR, MF_STRING,
        MF_UNCHECKED, MSG, SW_HIDE, SW_RESTORE, SW_SHOW, TPM_BOTTOMALIGN, TPM_LEFTALIGN,
        TPM_RIGHTBUTTON, WM_APP, WM_CLOSE, WM_COMMAND, WM_DESTROY, WM_LBUTTONDBLCLK,
        WM_LBUTTONUP, WM_NULL, WM_RBUTTONUP, WNDCLASSW,
    };

    const CLASS_NAME: &str = "DeviceHubTrayClass";
    const MUTEX_NAME: &str = "Local\\DeviceHub_SingleInstance_Mutex";
    const WM_TRAY: u32 = WM_APP + 1;
    const WM_SHOW_EXISTING: u32 = WM_APP + 2;

    const CMD_OPEN: usize = 1001;
    const CMD_AUTOSTART: usize = 1002;
    const CMD_QUIT: usize = 1003;

    static MAIN_HWND: AtomicIsize = AtomicIsize::new(0);
    static TRAY_HWND: AtomicIsize = AtomicIsize::new(0);
    static ALLOW_QUIT: AtomicBool = AtomicBool::new(false);
    static MINIMIZE_TO_TRAY: AtomicBool = AtomicBool::new(true);
    static INITIALIZED: AtomicBool = AtomicBool::new(false);
    static MUTEX_HANDLE: AtomicIsize = AtomicIsize::new(0);

    fn wide(s: &str) -> Vec<u16> {
        OsStr::new(s).encode_wide().chain(Some(0)).collect()
    }

    pub fn acquire_single_instance() -> bool {
        unsafe {
            let name = wide(MUTEX_NAME);
            let h = CreateMutexW(std::ptr::null(), 1, name.as_ptr());
            if h.is_null() {
                return true;
            }
            if GetLastError() == ERROR_ALREADY_EXISTS {
                let cls = wide(CLASS_NAME);
                let existing = FindWindowW(cls.as_ptr(), std::ptr::null());
                if !existing.is_null() {
                    PostMessageW(existing, WM_SHOW_EXISTING, 0, 0);
                }
                return false;
            }
            MUTEX_HANDLE.store(h as isize, Ordering::SeqCst);
            true
        }
    }

    pub fn show_main_window() {
        let raw = MAIN_HWND.load(Ordering::SeqCst);
        if raw == 0 {
            return;
        }
        unsafe {
            let hwnd = raw as HWND;
            ShowWindow(hwnd, SW_SHOW);
            ShowWindow(hwnd, SW_RESTORE);
            SetForegroundWindow(hwnd);
        }
    }

    pub fn should_close_window() -> bool {
        if ALLOW_QUIT.load(Ordering::SeqCst) || !MINIMIZE_TO_TRAY.load(Ordering::SeqCst) {
            return true;
        }
        let raw = MAIN_HWND.load(Ordering::SeqCst);
        if raw != 0 {
            unsafe {
                ShowWindow(raw as HWND, SW_HIDE);
            }
        }
        false
    }

    pub fn set_minimize_to_tray(on: bool) {
        MINIMIZE_TO_TRAY.store(on, Ordering::SeqCst);
    }

    pub fn request_quit() {
        ALLOW_QUIT.store(true, Ordering::SeqCst);
        remove_tray_icon();
        let raw = MAIN_HWND.load(Ordering::SeqCst);
        if raw != 0 {
            unsafe {
                PostMessageW(raw as HWND, WM_CLOSE, 0, 0);
            }
        }
    }

    pub fn shutdown() {
        remove_tray_icon();
        let tray = TRAY_HWND.swap(0, Ordering::SeqCst);
        if tray != 0 {
            unsafe {
                PostMessageW(tray as HWND, WM_CLOSE, 0, 0);
            }
        }
    }

    fn remove_tray_icon() {
        let tray = TRAY_HWND.load(Ordering::SeqCst);
        if tray == 0 {
            return;
        }
        unsafe {
            let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
            nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
            nid.hWnd = tray as HWND;
            nid.uID = 1;
            Shell_NotifyIconW(NIM_DELETE, &nid);
        }
    }

    unsafe fn create_accent_icon() -> HICON {
        const W: i32 = 16;
        const H: i32 = 16;
        let mut pixels = [0u32; (W * H) as usize];
        // Simple crisp cyan/blue rounded badge with white inner ring
        for y in 0..H {
            for x in 0..W {
                let dx = x as f32 - 7.5;
                let dy = y as f32 - 7.5;
                let r = (dx * dx + dy * dy).sqrt();
                if r <= 7.2 {
                    pixels[(y * W + x) as usize] = if (3.0..=4.7).contains(&r) {
                        0xFFFFFFFF
                    } else {
                        0xFF288CFF
                    };
                }
            }
        }
        let color_bmp: HBITMAP = CreateBitmap(W, H, 1, 32, pixels.as_ptr() as *const _);
        let mask_bits = [0u8; 32];
        let mask_bmp: HBITMAP = CreateBitmap(W, H, 1, 1, mask_bits.as_ptr() as *const _);
        let ii = ICONINFO {
            fIcon: 1,
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: mask_bmp,
            hbmColor: color_bmp,
        };
        let icon = CreateIconIndirect(&ii);
        if !color_bmp.is_null() {
            DeleteObject(color_bmp as _);
        }
        if !mask_bmp.is_null() {
            DeleteObject(mask_bmp as _);
        }
        icon
    }

    unsafe extern "system" fn tray_wnd_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match msg {
            WM_SHOW_EXISTING => {
                show_main_window();
                0
            }
            WM_TRAY => {
                let ev = (lparam & 0xFFFF) as u32;
                if ev == WM_LBUTTONUP || ev == WM_LBUTTONDBLCLK {
                    show_main_window();
                } else if ev == WM_RBUTTONUP {
                    let menu = CreatePopupMenu();
                    if !menu.is_null() {
                        let open_txt = wide("Device HUB を開く");
                        let auto_txt = wide("Windows 起動時に自動起動");
                        let quit_txt = wide("終了");
                        AppendMenuW(menu, MF_STRING, CMD_OPEN, open_txt.as_ptr());
                        let auto_flag = if crate::autostart::is_enabled() {
                            MF_STRING | MF_CHECKED
                        } else {
                            MF_STRING | MF_UNCHECKED
                        };
                        AppendMenuW(menu, auto_flag, CMD_AUTOSTART, auto_txt.as_ptr());
                        AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
                        AppendMenuW(menu, MF_STRING, CMD_QUIT, quit_txt.as_ptr());

                        let mut pt = POINT { x: 0, y: 0 };
                        GetCursorPos(&mut pt);
                        SetForegroundWindow(hwnd);
                        TrackPopupMenu(
                            menu,
                            TPM_LEFTALIGN | TPM_BOTTOMALIGN | TPM_RIGHTBUTTON,
                            pt.x,
                            pt.y,
                            0,
                            hwnd,
                            std::ptr::null(),
                        );
                        PostMessageW(hwnd, WM_NULL, 0, 0);
                        DestroyMenu(menu);
                    }
                }
                0
            }
            WM_COMMAND => {
                let id = wparam & 0xFFFF;
                match id {
                    CMD_OPEN => show_main_window(),
                    CMD_AUTOSTART => {
                        let cur = crate::autostart::is_enabled();
                        let _ = crate::autostart::set_enabled(!cur);
                    }
                    CMD_QUIT => request_quit(),
                    _ => {}
                }
                0
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                0
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }

    pub fn init(main_hwnd: isize, start_hidden: bool, minimize_to_tray: bool) {
        MAIN_HWND.store(main_hwnd, Ordering::SeqCst);
        MINIMIZE_TO_TRAY.store(minimize_to_tray, Ordering::SeqCst);
        if start_hidden && main_hwnd != 0 {
            unsafe {
                ShowWindow(main_hwnd as HWND, SW_HIDE);
            }
        }
        if INITIALIZED.swap(true, Ordering::SeqCst) {
            return;
        }

        std::thread::Builder::new()
            .name("device-hub-tray".into())
            .spawn(move || unsafe {
                let hinst = GetModuleHandleW(std::ptr::null());
                let cls = wide(CLASS_NAME);
                let wc = WNDCLASSW {
                    style: 0,
                    lpfnWndProc: Some(tray_wnd_proc),
                    cbClsExtra: 0,
                    cbWndExtra: 0,
                    hInstance: hinst,
                    hIcon: std::ptr::null_mut(),
                    hCursor: std::ptr::null_mut(),
                    hbrBackground: std::ptr::null_mut(),
                    lpszMenuName: std::ptr::null(),
                    lpszClassName: cls.as_ptr(),
                };
                RegisterClassW(&wc);
                let hwnd = CreateWindowExW(
                    0,
                    cls.as_ptr(),
                    cls.as_ptr(),
                    0,
                    0,
                    0,
                    0,
                    0,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    hinst,
                    std::ptr::null(),
                );
                if hwnd.is_null() {
                    return;
                }
                TRAY_HWND.store(hwnd as isize, Ordering::SeqCst);

                let icon = create_accent_icon();
                let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
                nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
                nid.hWnd = hwnd;
                nid.uID = 1;
                nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
                nid.uCallbackMessage = WM_TRAY;
                nid.hIcon = icon;
                let tip = wide("Device HUB");
                let n = tip.len().min(nid.szTip.len());
                nid.szTip[..n].copy_from_slice(&tip[..n]);
                Shell_NotifyIconW(NIM_ADD, &nid);

                let mut msg: MSG = std::mem::zeroed();
                while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }

                Shell_NotifyIconW(NIM_DELETE, &nid);
                if !icon.is_null() {
                    DestroyIcon(icon);
                }
            })
            .ok();
    }
}

#[cfg(not(windows))]
mod win {
    pub fn acquire_single_instance() -> bool {
        true
    }
    pub fn init(_main_hwnd: isize, _start_hidden: bool, _minimize_to_tray: bool) {}
    pub fn should_close_window() -> bool {
        true
    }
    pub fn set_minimize_to_tray(_on: bool) {}
    pub fn request_quit() {}
    pub fn shutdown() {}
}

pub use win::{
    acquire_single_instance, init, request_quit, set_minimize_to_tray, should_close_window,
    shutdown,
};
