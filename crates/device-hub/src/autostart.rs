//! Windows スタートアップ自動起動 (`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`)。

#[cfg(windows)]
mod win {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
        HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_SZ,
    };

    const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
    const VALUE_NAME: &str = "DeviceHub";

    fn wide(s: &str) -> Vec<u16> {
        OsStr::new(s).encode_wide().chain(Some(0)).collect()
    }

    pub fn is_enabled() -> bool {
        unsafe {
            let key_w = wide(RUN_KEY);
            let val_w = wide(VALUE_NAME);
            let mut hkey = std::ptr::null_mut();
            if RegOpenKeyExW(HKEY_CURRENT_USER, key_w.as_ptr(), 0, KEY_READ, &mut hkey)
                != ERROR_SUCCESS
            {
                return false;
            }
            let mut ty = 0u32;
            let mut size = 0u32;
            let rc = RegQueryValueExW(
                hkey,
                val_w.as_ptr(),
                std::ptr::null_mut(),
                &mut ty,
                std::ptr::null_mut(),
                &mut size,
            );
            RegCloseKey(hkey);
            rc == ERROR_SUCCESS && ty == REG_SZ && size > 2
        }
    }

    pub fn set_enabled(enable: bool) -> Result<(), String> {
        unsafe {
            let key_w = wide(RUN_KEY);
            let val_w = wide(VALUE_NAME);
            let mut hkey = std::ptr::null_mut();
            let rc = RegOpenKeyExW(HKEY_CURRENT_USER, key_w.as_ptr(), 0, KEY_WRITE, &mut hkey);
            if rc != ERROR_SUCCESS {
                return Err(format!("RegOpenKeyExW failed: {rc}"));
            }
            let res = if enable {
                let exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
                let cmd = format!("\"{}\" --background", exe.display());
                let data_w = wide(&cmd);
                let byte_len = (data_w.len() * 2) as u32;
                let wrc = RegSetValueExW(
                    hkey,
                    val_w.as_ptr(),
                    0,
                    REG_SZ,
                    data_w.as_ptr() as *const u8,
                    byte_len,
                );
                if wrc == ERROR_SUCCESS {
                    Ok(())
                } else {
                    Err(format!("RegSetValueExW failed: {wrc}"))
                }
            } else {
                let drc = RegDeleteValueW(hkey, val_w.as_ptr());
                // ERROR_FILE_NOT_FOUND (2) is fine when already disabled.
                if drc == ERROR_SUCCESS || drc == 2 {
                    Ok(())
                } else {
                    Err(format!("RegDeleteValueW failed: {drc}"))
                }
            };
            RegCloseKey(hkey);
            res
        }
    }
}

#[cfg(not(windows))]
mod win {
    pub fn is_enabled() -> bool {
        false
    }
    pub fn set_enabled(_enable: bool) -> Result<(), String> {
        Ok(())
    }
}

pub use win::{is_enabled, set_enabled};
