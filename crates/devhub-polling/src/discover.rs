//! USBPcap 対象の自動発見 — ハブ番号/アドレスの手入力を製品選択に置き換える。
//!
//! 方針:
//! 1. `\\.\USBPcap{n}` をアクセス権 0 で開き `IOCTL_USBPCAP_GET_HUB_SYMLINK` で
//!    対応するルートハブのデバイスオブジェクト名を得る (権限不要 —
//!    ドライバは FILE_READ_ATTRIBUTES のみのハンドルにこの IOCTL を許す)。
//! 2. 構成マネージャ (cfgmgr32) で `USB` 列挙子のデバイス一覧を取り、
//!    `USB\ROOT_HUB*` ノードの PDO 名とシンボリックリンクを照合して
//!    どのルートハブがどの USBPcapN かを決める。
//! 3. 複合デバイス (`USB\VID_*&PID_*\*`) の親をたどって対象ルートハブに
//!    ぶら下がるものを集め、アドレス・製品名・接続場所を返す。
//! 4. HID インターフェースの usage でマウス判定 (UsagePage=1, Usage=2) を付ける。

use std::io;

use crate::UsbDevice;

use windows_sys::core::GUID;
use windows_sys::Win32::Devices::DeviceAndDriverInstallation::{
    CM_Get_Device_IDW, CM_Get_Device_ID_ListW, CM_Get_Device_ID_List_SizeW,
    CM_Get_DevNode_PropertyW, CM_Get_Parent, CM_Locate_DevNodeW,
    SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInterfaces, SetupDiGetClassDevsW,
    SetupDiGetDeviceInterfaceDetailW, CM_GETIDLIST_FILTER_ENUMERATOR,
    CM_GETIDLIST_FILTER_PRESENT, CM_LOCATE_DEVNODE_NORMAL, CR_BUFFER_SMALL, CR_SUCCESS,
    DIGCF_DEVICEINTERFACE, DIGCF_PRESENT, HDEVINFO, SP_DEVICE_INTERFACE_DATA,
    SP_DEVICE_INTERFACE_DETAIL_DATA_W, SP_DEVINFO_DATA,
};
use windows_sys::Win32::Devices::HumanInterfaceDevice::{
    HidD_FreePreparsedData, HidD_GetPreparsedData, HidP_GetCaps, GUID_DEVINTERFACE_HID,
    HIDP_CAPS, HIDP_STATUS_SUCCESS, PHIDP_PREPARSED_DATA,
};
use windows_sys::Win32::Devices::Properties::{
    DEVPROP_TYPE_STRING, DEVPROP_TYPE_UINT32, DEVPKEY_Device_Address,
    DEVPKEY_Device_BusReportedDeviceDesc, DEVPKEY_Device_FriendlyName,
    DEVPKEY_Device_LocationInfo,
};
use windows_sys::Win32::Foundation::{DEVPROPKEY, HWND};

/// `\\.\USBPcap{n}` が握るルートハブのデバイスオブジェクト名。
/// アクセス権 0 の open で済むので非管理者でも呼べる。
pub fn hub_symlink(hub: u16) -> io::Result<String> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::System::IO::DeviceIoControl;
    let file = std::fs::OpenOptions::new()
        .access_mode(0)
        .share_mode(3)
        .open(format!(r"\\.\USBPcap{hub}"))?;
    let mut buf = vec![0u16; 1024];
    let mut returned = 0u32;
    // USBPcapDriver/include/USBPcap.h — CTL_CODE(FILE_DEVICE_UNKNOWN, 0x803,
    // METHOD_BUFFERED, FILE_ANY_ACCESS).
    let ok = unsafe {
        DeviceIoControl(
            file.as_raw_handle(),
            0x0022_200c,
            std::ptr::null(),
            0,
            buf.as_mut_ptr().cast(),
            (buf.len() * 2) as u32,
            &mut returned,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    let chars = returned as usize / 2;
    Ok(String::from_utf16_lossy(&buf[..chars.min(buf.len())])
        .trim_end_matches('\0')
        .to_string())
}

use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::AsRawHandle;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn from_wide(w: &[u16]) -> String {
    let end = w.iter().position(|&c| c == 0).unwrap_or(w.len());
    String::from_utf16_lossy(&w[..end])
}

/// `USB` 列挙子配下の全デバイスインスタンス ID。
fn enumerator_ids() -> Vec<String> {
    let filter = wide("USB");
    unsafe {
        let mut len = 0u32;
        if CM_Get_Device_ID_List_SizeW(&mut len, filter.as_ptr(), CM_GETIDLIST_FILTER_ENUMERATOR)
            != CR_SUCCESS
            || len == 0
        {
            return Vec::new();
        }
        let mut buf = vec![0u16; len as usize];
        if CM_Get_Device_ID_ListW(
            filter.as_ptr(),
            buf.as_mut_ptr(),
            len,
            CM_GETIDLIST_FILTER_ENUMERATOR | CM_GETIDLIST_FILTER_PRESENT,
        ) != CR_SUCCESS
        {
            return Vec::new();
        }
        buf.split(|&c| c == 0)
            .filter(|s| !s.is_empty())
            .map(String::from_utf16_lossy)
            .collect()
    }
}

fn locate(id: &str) -> Option<u32> {
    let wide_id = wide(id);
    let mut node = 0;
    unsafe {
        (CM_Locate_DevNodeW(&mut node, wide_id.as_ptr(), CM_LOCATE_DEVNODE_NORMAL) == CR_SUCCESS)
            .then_some(node)
    }
}

fn node_id(node: u32) -> Option<String> {
    let mut buf = [0u16; 512];
    unsafe {
        (CM_Get_Device_IDW(node, buf.as_mut_ptr(), buf.len() as u32, 0) == CR_SUCCESS)
            .then(|| from_wide(&buf))
    }
}

/// ノードの親を最大 `depth` 段たどり、各段のインスタンス ID を返す。
fn ancestors(node: u32, depth: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = node;
    for _ in 0..depth {
        let mut parent = 0;
        if unsafe { CM_Get_Parent(&mut parent, cur, 0) } != CR_SUCCESS {
            break;
        }
        match node_id(parent) {
            Some(id) => out.push(id),
            None => break,
        }
        cur = parent;
    }
    out
}

fn prop(node: u32, key: &DEVPROPKEY) -> Option<(Vec<u8>, u32)> {
    unsafe {
        let mut ty = 0u32;
        let mut size = 0u32;
        match CM_Get_DevNode_PropertyW(node, key, &mut ty, std::ptr::null_mut(), &mut size, 0) {
            CR_SUCCESS | CR_BUFFER_SMALL if size > 0 => {}
            _ => return None,
        }
        let mut buf = vec![0u8; size as usize];
        let mut ty = 0u32;
        (CM_Get_DevNode_PropertyW(node, key, &mut ty, buf.as_mut_ptr(), &mut size, 0)
            == CR_SUCCESS)
            .then_some((buf, ty))
    }
}

fn prop_string(node: u32, key: &DEVPROPKEY) -> Option<String> {
    let (buf, ty) = prop(node, key)?;
    if ty != DEVPROP_TYPE_STRING {
        return None;
    }
    let wide: Vec<u16> = buf
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    Some(from_wide(&wide))
}

fn prop_u32(node: u32, key: &DEVPROPKEY) -> Option<u32> {
    let mut ty = 0u32;
    let mut size = 4u32;
    let mut v = 0u32;
    unsafe {
        (CM_Get_DevNode_PropertyW(
            node,
            key,
            &mut ty,
            (&mut v as *mut u32).cast(),
            &mut size,
            0,
        ) == CR_SUCCESS
            && ty == DEVPROP_TYPE_UINT32)
            .then_some(v)
    }
}

/// `USB\VID_33E4&PID_3854\ser` から (vid, pid) を取る。
fn parse_vid_pid(id: &str) -> Option<(u16, u16)> {
    let seg = id.split('\\').nth(1)?;
    let vid = seg.strip_prefix("VID_")?.get(..4)?;
    let pid = seg.split("PID_").nth(1)?.get(..4)?;
    Some((
        u16::from_str_radix(vid, 16).ok()?,
        u16::from_str_radix(pid, 16).ok()?,
    ))
}

// The PnP address identifies a port on the immediate parent hub. USBPcap
// records carry a bus address, assigned independently by the USB stack.
fn bus_address(parent: &str, port: u32, vid: u16, pid: u16) -> Option<u8> {
    use windows_sys::Win32::Devices::DeviceAndDriverInstallation::{
        CM_Get_Device_Interface_ListW, CM_GET_DEVICE_INTERFACE_LIST_PRESENT,
    };
    use windows_sys::Win32::Devices::Usb::*;
    use windows_sys::Win32::System::IO::DeviceIoControl;
    let mut paths = [0u16; 4096];
    let rc = unsafe { CM_Get_Device_Interface_ListW(&GUID_DEVINTERFACE_USB_HUB,
        wide(parent).as_ptr(), paths.as_mut_ptr(), paths.len() as u32,
        CM_GET_DEVICE_INTERFACE_LIST_PRESENT) };
    if rc != CR_SUCCESS { return None; }
    let path = from_wide(&paths);
    let file = std::fs::OpenOptions::new().access_mode(0x4000_0000).share_mode(3)
        .open(&path).or_else(|_| std::fs::OpenOptions::new().access_mode(0).share_mode(3).open(path)).ok()?;
    let mut buf = [0u8; 4096];
    let mut n = 0;
    let ok = unsafe { DeviceIoControl(file.as_raw_handle(), IOCTL_USB_GET_NODE_CONNECTION_INFORMATION_EX,
        (&port as *const u32).cast(), 4, buf.as_mut_ptr().cast(), buf.len() as u32,
        &mut n, std::ptr::null_mut()) };
    if ok == 0 || (n as usize) < std::mem::offset_of!(USB_NODE_CONNECTION_INFORMATION_EX, PipeList) { return None; }
    let conn = unsafe { buf.as_ptr().cast::<USB_NODE_CONNECTION_INFORMATION_EX>().read_unaligned() };
    (conn.ConnectionStatus == DeviceConnected && conn.DeviceDescriptor.idVendor == vid
        && conn.DeviceDescriptor.idProduct == pid && (1..=127).contains(&{conn.DeviceAddress}))
        .then_some(conn.DeviceAddress as u8)
}

fn norm(s: &str) -> String {
    s.trim_end_matches('\0')
        .replace('#', "\\")
        .to_ascii_uppercase()
}

/// マウス usage (Page 1 / Usage 2) の HID コレクションを持つ
/// 複合デバイス (`USB\VID_*&PID_*\*` インスタンス ID) の集合。
fn mouse_parents() -> std::collections::HashSet<String> {
    let mut set = std::collections::HashSet::new();
    unsafe {
        let info = SetupDiGetClassDevsW(
            &GUID_DEVINTERFACE_HID,
            std::ptr::null(),
            HWND::default(),
            DIGCF_PRESENT | DIGCF_DEVICEINTERFACE,
        );
        if info == HDEVINFO::default() || info == -1isize {
            return set;
        }
        let mut index = 0u32;
        loop {
            let mut data = SP_DEVICE_INTERFACE_DATA {
                cbSize: std::mem::size_of::<SP_DEVICE_INTERFACE_DATA>() as u32,
                InterfaceClassGuid: GUID { data1: 0, data2: 0, data3: 0, data4: [0; 8] },
                Flags: 0,
                Reserved: 0,
            };
            if SetupDiEnumDeviceInterfaces(
                info,
                std::ptr::null(),
                &GUID_DEVINTERFACE_HID,
                index,
                &mut data,
            ) == 0
            {
                break;
            }
            index += 1;
            // 詳細 (デバイスパス + デバイスノード) を取る。
            let mut need = 0u32;
            SetupDiGetDeviceInterfaceDetailW(
                info,
                &data,
                std::ptr::null_mut(),
                0,
                &mut need,
                std::ptr::null_mut(),
            );
            if need < std::mem::size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32 || need > 4096 {
                continue;
            }
            let mut buf = vec![0u32; (need as usize).div_ceil(4)];
            let detail = buf.as_mut_ptr() as *mut SP_DEVICE_INTERFACE_DETAIL_DATA_W;
            (*detail).cbSize =
                std::mem::size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32;
            let mut dev = SP_DEVINFO_DATA {
                cbSize: std::mem::size_of::<SP_DEVINFO_DATA>() as u32,
                ..std::mem::zeroed()
            };
            if SetupDiGetDeviceInterfaceDetailW(
                info,
                &data,
                detail,
                need,
                std::ptr::null_mut(),
                &mut dev,
            ) == 0
            {
                continue;
            }
            let path = {
                let raw = std::slice::from_raw_parts(
                    (*detail).DevicePath.as_ptr(),
                    (need as usize - 4) / 2,
                );
                from_wide(raw)
            };
            // usage がマウスか確認 — 開けない (権限不足のキーボード等) は飛ばす。
            let file = std::fs::OpenOptions::new()
                .read(true)
                .open(&path)
                .or_else(|_| {
                    std::fs::OpenOptions::new()
                        .access_mode(0)
                        .open(&path)
                });
            let is_mouse = file.ok().is_some_and(|f| {
                let mut preparsed: PHIDP_PREPARSED_DATA = 0;
                if !HidD_GetPreparsedData(f.as_raw_handle(), &mut preparsed) {
                    return false;
                }
                let mut caps: HIDP_CAPS = std::mem::zeroed();
                let ok = HidP_GetCaps(preparsed, &mut caps) == HIDP_STATUS_SUCCESS;
                HidD_FreePreparsedData(preparsed);
                ok && caps.UsagePage == 1 && caps.Usage == 2
            });
            if !is_mouse {
                continue;
            }
            // HID コレクション → MI インターフェース → 複合デバイスの順に親をたどる。
            for id in ancestors(dev.DevInst, 4) {
                if id.to_ascii_uppercase().starts_with("USB\\VID_")
                    && !id.to_ascii_uppercase().contains("&MI_")
                {
                    set.insert(norm(&id));
                    break;
                }
            }
        }
        SetupDiDestroyDeviceInfoList(info);
    }
    set
}

/// 現在の USBPcap ハブ列挙 — `\\.\USBPcap{n}` とルートハブの対応表。
/// 返り値: (usbpcap 番号, ルートハブの PnP インスタンス ID)。
pub fn hubs() -> Vec<(u16, String)> {
    let mut out = Vec::new();
    for n in 1..=16u16 {
        let Ok(link) = hub_symlink(n) else { continue };
        if link.is_empty() {
            continue;
        }
        out.push((n, norm(&link)));
    }
    out
}

/// 現在挿さっている USB デバイスの一覧 (USBPcap ハブ別)。
/// 管理者権限は不要。返り値は製品名→VID/PID の順。
pub fn devices() -> Vec<UsbDevice> {
    let mut out = Vec::new();
    // 1) USBPcapN → ルートハブのデバイスオブジェクト名。
    let links = hubs();
    if links.is_empty() {
        return out;
    }
    // 2) USB 列挙子の全インスタンス。ROOT_HUB ノードはシンボリックリンク
    //    (`\??\USB\ROOT_HUB30\<instance>\{GUID}`) にインスタンス ID を含むので
    //    そのまま照合できる。
    let ids = enumerator_ids();
    let mut roots: Vec<(u16, String)> = Vec::new(); // (usbpcap n, roothub instance id)
    for id in &ids {
        if !id.to_ascii_uppercase().starts_with("USB\\ROOT_HUB") {
            continue;
        }
        let nid = norm(id);
        if let Some((n, _)) = links.iter().find(|(_, link)| link.contains(&nid)) {
            roots.push((*n, id.clone()));
        }
    }
    // 3) 複合デバイス (USB\VID_*&PID_*\*) の親がどのルートハブか判定して集める。
    let mice = mouse_parents();
    for id in ids {
        let upper = id.to_ascii_uppercase();
        if !upper.starts_with("USB\\VID_") || upper.contains("&MI_") {
            continue;
        }
        let Some(node) = locate(&id) else { continue };
        // 親チェーンからルートハブを探す (外部ハブ越しも拾うため 6 段)。
        let chain = ancestors(node, 6);
        let hub = roots
            .iter()
            .find(|(_, rid)| chain.iter().any(|a| a.eq_ignore_ascii_case(rid)))
            .map(|(n, _)| *n)
            .unwrap_or(0);
        let (vid, pid) = parse_vid_pid(&id).unwrap_or((0, 0));
        if hub == 0 { continue; }
        let Some(address) = chain.first().and_then(|parent| bus_address(parent,
            prop_u32(node, &DEVPKEY_Device_Address)?, vid, pid)) else { continue };
        let name = prop_string(node, &DEVPKEY_Device_BusReportedDeviceDesc)
            .or_else(|| prop_string(node, &DEVPKEY_Device_FriendlyName))
            .unwrap_or_else(|| id.clone());
        out.push(UsbDevice {
            hub,
            address,
            vid,
            pid,
            name,
            location: prop_string(node, &DEVPKEY_Device_LocationInfo).unwrap_or_default(),
            is_mouse: mice.contains(&norm(&id)),
            instance_id: id,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name).then(a.vid.cmp(&b.vid)).then(a.pid.cmp(&b.pid)));
    out
}

/// 保存した個体識別 (`instance_id`、無ければ VID/PID) から現接続の
/// ハブ/アドレスを再解決する。再接続でアドレスが変わっても追える。
pub fn resolve(instance_id: &str, vid: u16, pid: u16) -> Option<UsbDevice> {
    let mut all = devices().into_iter();
    if !instance_id.is_empty() {
        return all.find(|d| d.instance_id.eq_ignore_ascii_case(instance_id));
    }
    let mut matches = all.filter(|d| d.vid == vid && d.pid == pid);
    let first = matches.next()?;
    matches.next().is_none().then_some(first)
}
