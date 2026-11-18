//! 同じディレクトリの一意な一時ファイルへ書いてから置換する共通処理。
//! 書き込み途中の失敗で旧ファイルが消えず、固定名の一時ファイルを
//! 共有しないため同時保存どうしも衝突しない。失敗は呼び出し元に返す。

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

fn tmp_path(path: &Path) -> PathBuf {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let stem = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("tmp");
    path.with_file_name(format!(".{stem}.tmp.{pid}.{n}"))
}

/// `contents` を `path` へアトミックに書き込む (一時ファイル → rename)。
pub fn write(path: &Path, contents: &str) -> io::Result<()> {
    let tmp = tmp_path(path);
    let r = std::fs::write(&tmp, contents).and_then(|_| std::fs::rename(&tmp, path));
    if r.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    r
}
