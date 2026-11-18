//! USBPcap 実測を OBS の表示文字列に写す薄い境界。計測本体は devhub_polling —
//! アプリ側 (テスト画面) と同じ値が出る。
pub(crate) use devhub_polling::{Capture, Snapshot, Status, Target};

/// 直近 1 秒の転送数または状態を 1 行で。
pub(crate) fn label(s: &Snapshot) -> String {
    match s.status {
        Status::Waiting => "Waiting for mouse input".into(),
        Status::Live => format!("USB   {} Hz", s.hz),
        Status::DriverMissing { .. } => "USBPcap driver / hub not found".into(),
        Status::AccessDenied { .. } => "USBPcap access denied".into(),
        Status::HubBusy { .. } => "USBPcap hub already in use".into(),
        Status::Disconnected { .. } | Status::Failed { .. } => "USB capture stopped".into(),
    }
}
