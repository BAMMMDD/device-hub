//! ファームウェア更新通知の永続状態 (`%APPDATA%\DeviceHub\firmware.json`)。
//! キーは `DeviceInfo.model` (機種ごとに 1 件 — 同じ機種が複数つながっても共通)。

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// 自動確認の間隔。
pub const CHECK_EVERY: u64 = 24 * 60 * 60;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct FwEntry {
    /// 前回確認した配布物の指紋 (基準値)。
    pub fingerprint: String,
    /// 最後に確認を試みた時刻 (UNIX 秒)。失敗でも記録して打ちすぎない。
    pub last_check: u64,
    /// ユーザーが閉じた指紋 — 同じ更新では再通知しない。
    pub dismissed: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct FwState {
    pub devices: BTreeMap<String, FwEntry>,
}

fn path() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("DeviceHub").join("firmware.json"))
}

impl FwState {
    pub fn load() -> Self {
        path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(p) = path() else { return };
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let tmp = p.with_extension("json.tmp");
            if std::fs::write(&tmp, s).is_ok() {
                let _ = std::fs::rename(&tmp, &p);
            }
        }
    }

    fn now() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }

    /// 自動確認すべき間隔が経過したか (force なら常に true)。
    pub fn due(&self, key: &str, force: bool) -> bool {
        if force {
            return true;
        }
        let last = self.devices.get(key).map(|e| e.last_check).unwrap_or(0);
        Self::now().saturating_sub(last) >= CHECK_EVERY
    }

    /// 確認を始める直前に打刻する (失敗しても 24h は再試行しない)。
    pub fn touch(&mut self, key: &str) {
        self.devices.entry(key.to_string()).or_default().last_check = Self::now();
    }
}
