//! USBPcap 経由の実測ポーリングレート計測 — OBS プラグインと Device HUB アプリの共通部品。
//!
//! マウスは「動きがあるときだけ」レポートを送るので、対象の interrupt IN 転送の
//! 完了数 ≒ 実際のデバイスポーリングレート (無線ドングルではレシーバー → PC 区間)。
//! レコードは USBPcap が classic little-endian pcap (LINKTYPE_USBPCAP) で流す。
//! レコード形式: <https://desowin.org/usbpcap/captureformat.html>
//!
//! 使い方:
//! 1. `Capture::start(target)` — 別スレッドで `\\.\USBPcap{hub}` を開いて読み始める。
//!    ハブの open は排他 (`share_mode(0)`): 同じハブを掴めるのは全プロセスで 1 つ。
//! 2. `snapshot()` — 直近 1 秒の転送数、計測中の最大 1 秒区間、状態を取る。
//! 3. `stop()` / drop — ブロッキング read を解除してからワーカーを join する。

use std::collections::VecDeque;
use std::io::{self, Read};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

/// 計測対象。hub/device は接続のたびに変わり得る値 — 個体の永続識別
/// (VID/PID・シリアル等) はここに持たず、呼び出し側で紐付ける。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Target {
    /// USBPcap のハブ番号 (`\\.\USBPcap{n}` の n)。
    pub hub: u16,
    /// 現在の USB デバイスアドレス (1-127)。抜き差しで変わる。
    pub device: u8,
    /// 移動用 interrupt IN エンドポイント (IN ビット込み。例: 0x81)。
    pub endpoint: u8,
    /// 共有エンドポイントで絞る Report ID。None = 全件数える。
    pub report: Option<u8>,
}

/// 計測の状態。エラー系は終端状態で、OS コード (GetLastError) を残す。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    /// 開始直後 — 対象の転送をまだ 1 件も受けていない。
    Waiting,
    /// 受信中。`Snapshot::hz` は直近 1 秒の件数 (静止中は 0)。
    Live,
    /// USBPcap ドライバ未導入、またはその番号のハブが存在しない。
    DriverMissing { os: Option<i32> },
    /// ハブを読み書きモードで開く権限がない (管理者としての実行が必要)。
    AccessDenied { os: Option<i32> },
    /// 同じハブを別のプロセス/ソースが掴んでいる。
    HubBusy { os: Option<i32> },
    /// 受信途中でストリームが切れた (ハブ抜去・ドライバ停止など)。
    Disconnected { os: Option<i32> },
    /// 上記以外の失敗 (不正なストリーム・その他の I/O エラー)。
    Failed { os: Option<i32> },
}

impl Status {
    /// エラー系の終端状態か。
    pub fn is_error(self) -> bool {
        matches!(
            self,
            Self::DriverMissing { .. }
                | Self::AccessDenied { .. }
                | Self::HubBusy { .. }
                | Self::Disconnected { .. }
                | Self::Failed { .. }
        )
    }
}

/// `snapshot()` の 1 回分の値。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Snapshot {
    pub status: Status,
    /// 直近 1 秒に完了した対象転送の数 (受信済みで静止中は 0)。
    pub hz: u32,
    /// 開始から観測した任意 1 秒区間 `(t-1s, t]` の最大件数。
    /// 表示更新周期ではなく受信ごとに更新するので、途中の山を取りこぼさない。
    pub peak_hz: u32,
}

/// この環境で USB 計測を試せるか (非 Windows では常に失敗する)。
pub fn supported() -> bool {
    cfg!(windows)
}

/// 計測対象として提示する USB デバイス 1 台 (`discover::devices` が返す)。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct UsbDevice {
    /// `\\.\USBPcap{n}` の n。0 はルートハブとの照合が取れなかったもの。
    pub hub: u16,
    /// 現在の USB アドレス。抜き差し・ポート変更で変わる。
    pub address: u8,
    pub vid: u16,
    pub pid: u16,
    /// デバイスが自己申告する製品名 (BusReportedDeviceDesc)。無ければ FriendlyName。
    pub name: String,
    /// PnP インスタンス ID (`USB\VID_xxxx&PID_yyyy\<serial>`)。再接続後の再解決に使う。
    pub instance_id: String,
    /// `Port_#0002.Hub_#0001` のような接続場所。
    pub location: String,
    /// マウスの HID コレクション (UsagePage=1, Usage=2) を持つ。
    pub is_mouse: bool,
}

impl UsbDevice {
    /// 計測開始に使える `Target` を組み立てる。
    pub fn target(&self, endpoint: u8, report: Option<u8>) -> Target {
        Target {
            hub: self.hub,
            device: self.address,
            endpoint,
            report,
        }
    }

    /// ピッカー表示用の短い説明。
    pub fn label(&self) -> String {
        format!(
            "{name} ({vid:04x}:{pid:04x}) {loc}",
            name = self.name,
            vid = self.vid,
            pid = self.pid,
            loc = self.location
        )
    }
}

#[cfg(windows)]
pub mod discover;

#[cfg(not(windows))]
/// `discover` の非 Windows スタブ。常に空を返す。
pub mod discover {
    use super::UsbDevice;
    use std::io;
    pub fn devices() -> Vec<UsbDevice> {
        Vec::new()
    }
    pub fn hubs() -> Vec<(u16, String)> {
        Vec::new()
    }
    pub fn hub_symlink(_: u16) -> io::Result<String> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "USBPcap requires Windows",
        ))
    }
    pub fn resolve(_: &str, _: u16, _: u16) -> Option<UsbDevice> {
        None
    }
}

#[derive(Default)]
struct Samples {
    /// 直近 1 秒の対象レコード時刻 (キャプチャ時刻 µs)。
    stamps: VecDeque<u64>,
    seen: bool,
    peak: u32,
    /// 終端のエラー状態 (Some になった時点でワーカーは終わっている)。
    error: Option<Status>,
}

impl Samples {
    fn expire(&mut self, now: u64) {
        let cutoff = now.saturating_sub(1_000_000);
        while self.stamps.front().is_some_and(|&t| t <= cutoff) {
            self.stamps.pop_front();
        }
    }

    fn snapshot(&mut self, now: u64) -> Snapshot {
        self.expire(now);
        Snapshot {
            status: self.error.unwrap_or(if self.seen {
                Status::Live
            } else {
                Status::Waiting
            }),
            hz: self.stamps.len() as u32,
            peak_hz: self.peak,
        }
    }
}

fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "Invalid USBPcap stream")
}

fn now_us() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64
}

/// pcap ストリームを読み、対象の成功した interrupt IN 完了 (データあり、
/// Report ID 指定時は一致) だけを数える。時刻はキャプチャ時刻を使う。
fn consume(
    mut input: impl Read,
    target: Target,
    stop: &AtomicBool,
    samples: &Mutex<Samples>,
) -> io::Result<()> {
    let mut global = [0; 24];
    input.read_exact(&mut global)?;
    if global[..8] != [0xd4, 0xc3, 0xb2, 0xa1, 2, 0, 4, 0] || global[20..] != 249u32.to_le_bytes() {
        return Err(invalid());
    }
    let mut packet = [0; 1024];
    while !stop.load(Ordering::Acquire) {
        let mut header = [0; 16];
        input.read_exact(&mut header)?;
        let size = u32::from_le_bytes(header[8..12].try_into().unwrap()) as usize;
        if !(27..=packet.len()).contains(&size) {
            // 対象外のサイズの転送 (短い制御レコード・他デバイスの大きな
            // report など) が混ざっても計測を止めない — 本体を捨てて進む。
            io::copy(&mut input.by_ref().take(size as u64), &mut io::sink())?;
            continue;
        }
        let data = packet.get_mut(..size).ok_or_else(invalid)?;
        input.read_exact(data)?;
        let offset = u16::from_le_bytes([data[0], data[1]]) as usize;
        if offset < 27
            || offset >= size
            || data[10..14] != [0; 4]
            || data[16] & 1 == 0
            || data[19..21] != u16::from(target.device).to_le_bytes()
            || data[21] != target.endpoint
            || data[22] != 1
            || data[23..27] == [0; 4]
            || target.report.is_some_and(|id| id != data[offset])
        {
            continue;
        }
        let sec = u32::from_le_bytes(header[..4].try_into().unwrap());
        let us = u32::from_le_bytes(header[4..8].try_into().unwrap());
        let stamp = u64::from(sec) * 1_000_000 + u64::from(us);
        let mut s = samples.lock().unwrap_or_else(|e| e.into_inner());
        s.expire(stamp);
        s.seen = true;
        s.stamps.push_back(stamp);
        // (t - 1秒, t] の件数を受信のたびに最大へ反映 — 表示周期でだけ
        // 最大を取ると、更新と更新の間の山を取りこぼす。
        s.peak = s.peak.max(s.stamps.len() as u32);
    }
    Ok(())
}

/// 失敗が起きた段階。open/ioctl と受信ループで OS コードの意味が違う。
#[derive(Clone, Copy, Debug)]
enum Stage {
    Open,
    Run,
}

/// `io::Error` をユーザー向けの終端状態に写す。
fn status_of(err: &io::Error, stage: Stage) -> Status {
    let os = err.raw_os_error();
    match (stage, os) {
        // ERROR_FILE_NOT_FOUND / ERROR_PATH_NOT_FOUND — ドライバ無しか番号違い。
        (Stage::Open, Some(2 | 3)) => Status::DriverMissing { os },
        // ERROR_ACCESS_DENIED — ハブの読み書き open は管理者権限が要る。
        (Stage::Open, Some(5)) => Status::AccessDenied { os },
        // ERROR_SHARING_VIOLATION — share_mode(0) で開くため同時占有は 1 つ。
        (Stage::Open, Some(32)) => Status::HubBusy { os },
        // 受信中の切断: EOF (ドライバがキャプチャを止めた) / NOT_CONNECTED / GEN_FAILURE。
        (Stage::Run, Some(2 | 3 | 31 | 1167)) => Status::Disconnected { os },
        (Stage::Run, _) if err.kind() == io::ErrorKind::UnexpectedEof => {
            Status::Disconnected { os }
        }
        _ => Status::Failed { os },
    }
}

/// 診断ログ — `DEVHUB_USB_LOG` にファイルパスがあればワーカーの
/// open/run/終了とエラーの遷移を記録する (未設定なら何もしない)。
#[cfg(windows)]
fn diag(msg: &str) {
    let Ok(path) = std::env::var("DEVHUB_USB_LOG") else {
        return;
    };
    let stamp = std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    if let Ok(mut f) = std::fs::OpenOptions::new().append(true).create(true).open(&path) {
        use std::io::Write;
        let _ = writeln!(f, "[{stamp}] {msg}");
    }
}

#[cfg(windows)]
fn capture(
    target: Target,
    stop: &AtomicBool,
    samples: &Mutex<Samples>,
) -> Result<(), (Stage, io::Error)> {
    use std::os::windows::{fs::OpenOptionsExt, io::AsRawHandle};
    use windows_sys::Win32::System::IO::DeviceIoControl;
    let stage = |e: io::Error| (Stage::Open, e);
    if target.hub == 0 || !(1..=127).contains(&target.device) || !(0x81..=0x8f).contains(&target.endpoint) {
        return Err((Stage::Open, io::Error::new(io::ErrorKind::InvalidInput, "invalid USB target")));
    }
    let path = format!(r"\\.\USBPcap{}", target.hub);
    let open = |share: u32| {
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .share_mode(share)
            .open(&path)
    };
    let file = match open(0) {
        Ok(f) => f,
        // 実機検証: USBPcap のドライバは占有中の 2 度目の open を共有違反 (32) では
        // なく STATUS_ACCESS_DENIED (5) で弾く。共有許可で開き直して区別する —
        // 32 が返れば他プロセスの占有 (HubBusy)、5 が返れば本当の権限不足。
        Err(e) if e.raw_os_error() == Some(5) => {
            return Err((
                Stage::Open,
                match open(0x3) {
                    Err(p) if p.raw_os_error() == Some(32) => {
                        io::Error::from_raw_os_error(32)
                    }
                    _ => e,
                },
            ));
        }
        Err(e) => return Err((Stage::Open, e)),
    };
    // USBPcapDriver/include/USBPcap.h: FILE_DEVICE_UNKNOWN, METHOD_BUFFERED.
    let ioctl = |code, input: &[u8]| {
        let mut returned = 0;
        let ok = unsafe {
            DeviceIoControl(
                file.as_raw_handle(),
                code,
                input.as_ptr().cast(),
                input.len() as u32,
                std::ptr::null_mut(),
                0,
                &mut returned,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    };
    ioctl(0x0022_6010, &64u32.to_le_bytes()).map_err(stage)?; // snapshot length
    ioctl(0x0022_6000, &(1u32 << 20).to_le_bytes()).map_err(stage)?; // buffer size
    let mut filter = [0u8; 17];
    filter[usize::from(target.device / 8)] |= 1 << (target.device % 8);
    ioctl(0x0022_e004, &filter).map_err(stage)?; // address filter
    consume(
        io::BufReader::with_capacity(65536, file),
        target,
        stop,
        samples,
    )
    .map_err(|e| (Stage::Run, e))
}

#[cfg(not(windows))]
fn capture(_: Target, _: &AtomicBool, _: &Mutex<Samples>) -> Result<(), (Stage, io::Error)> {
    Err((
        Stage::Open,
        io::Error::new(io::ErrorKind::Unsupported, "USBPcap requires Windows"),
    ))
}

/// 対象デバイスの interrupt IN 完了をエンドポイント別に集計する短時間スキャン。
/// EP 番号の自動検出に使う。実行中はハブを占有し、ハブの読み書き権限が要る。
/// 返り値は (EP, 件数) を件数の降順で並べたリスト。
#[cfg(windows)]
pub fn scan_interrupt(
    hub: u16,
    device: u8,
    dur: std::time::Duration,
) -> io::Result<Vec<(u8, u32)>> {
    use std::collections::BTreeMap;
    use std::os::windows::{fs::OpenOptionsExt, io::AsRawHandle};
    use windows_sys::Win32::System::IO::DeviceIoControl;
    if hub == 0 || !(1..=127).contains(&device) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid USB target"));
    }
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .share_mode(0)
        .open(format!(r"\\.\USBPcap{hub}"))?;
    let ioctl = |code, input: &[u8]| {
        let mut returned = 0;
        let ok = unsafe {
            DeviceIoControl(
                file.as_raw_handle(),
                code,
                input.as_ptr().cast(),
                input.len() as u32,
                std::ptr::null_mut(),
                0,
                &mut returned,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    };
    ioctl(0x0022_6010, &64u32.to_le_bytes())?;
    ioctl(0x0022_6000, &(1u32 << 20).to_le_bytes())?;
    let mut filter = [0u8; 17];
    filter[usize::from(device / 8)] |= 1 << (device % 8);
    ioctl(0x0022_e004, &filter)?;
    let counts = Arc::new(Mutex::new(BTreeMap::<u8, u32>::new()));
    let tally = counts.clone();
    let worker = std::thread::spawn(move || {
        let mut input = io::BufReader::with_capacity(65536, file);
        let mut global = [0; 24];
        if input.read_exact(&mut global).is_err() {
            return;
        }
        let mut packet = [0; 1024];
        loop {
            let mut header = [0; 16];
            if input.read_exact(&mut header).is_err() {
                return;
            }
            let size = u32::from_le_bytes(header[8..12].try_into().unwrap()) as usize;
            if !(27..=packet.len()).contains(&size) {
                // 対象外サイズの転送は捨てて進む (計測と同じ理由)。
                if io::copy(&mut input.by_ref().take(size as u64), &mut io::sink()).is_err() {
                    return;
                }
                continue;
            }
            let data = packet.get_mut(..size).unwrap();
            if input.read_exact(data).is_err() {
                return;
            }
            // 対象アドレスの成功した interrupt IN 完了 (データあり) だけ数える。
            if data[10..14] == [0; 4]
                && data[16] & 1 == 1
                && data[19..21] == u16::from(device).to_le_bytes()
                && (0x81..=0x8f).contains(&data[21])
                && data[22] == 1
                && data[23..27] != [0; 4]
            {
                *tally
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .entry(data[21])
                    .or_default() += 1;
            }
        }
    });
    std::thread::sleep(dur);
    while !worker.is_finished() {
        unsafe {
            windows_sys::Win32::System::IO::CancelSynchronousIo(worker.as_raw_handle());
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let _ = worker.join();
    let mut list: Vec<(u8, u32)> = counts
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .map(|(ep, n)| (*ep, *n))
        .collect();
    list.sort_by_key(|e| std::cmp::Reverse(e.1));
    Ok(list)
}

#[cfg(not(windows))]
pub fn scan_interrupt(_: u16, _: u8, _: std::time::Duration) -> io::Result<Vec<(u8, u32)>> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "USBPcap requires Windows",
    ))
}

/// 計測ハンドル。`start` でワーカー起動、`snapshot` で現在値、`stop`/drop で終了。
pub struct Capture {
    pub target: Target,
    samples: Arc<Mutex<Samples>>,
    cancel: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl Capture {
    pub fn start(target: Target) -> Self {
        let samples = Arc::new(Mutex::new(Samples::default()));
        let cancel = Arc::new(AtomicBool::new(false));
        let state = samples.clone();
        let flag = cancel.clone();
        let result = std::thread::Builder::new()
            .name("devhub-usb-rate".into())
            .spawn(move || {
                // 受信途中の切断は一過性のことが多い (デバイスの再列挙・
                // ドライバ側の取りこぼし等) — ハンドルを開き直して続行する。
                // 連続してすぐ失敗する場合だけ諦めて終端状態へ。
                let mut quick_retries = 0u32;
                loop {
                    let began = std::time::Instant::now();
                    #[cfg(windows)]
                    diag("capture: start");
                    match capture(target, &flag, &state) {
                        Ok(_) => break,
                        Err((stage, err)) => {
                            // 自分たちの停止要求による read 中断はエラーにしない。
                            if flag.load(Ordering::Acquire) {
                                break;
                            }
                            #[cfg(windows)]
                            diag(&format!(
                                "capture: {stage:?} error {} (os {:?})",
                                err,
                                err.raw_os_error()
                            ));
                            // Open 段の失敗 (権限・占有・未導入) は即終端。
                            if matches!(stage, Stage::Open) || quick_retries >= 20 {
                                state.lock().unwrap_or_else(|e| e.into_inner()).error =
                                    Some(status_of(&err, stage));
                                break;
                            }
                            quick_retries = if began.elapsed()
                                < std::time::Duration::from_secs(2)
                            {
                                quick_retries + 1
                            } else {
                                0
                            };
                            std::thread::sleep(std::time::Duration::from_millis(300));
                        }
                    }
                }
            });
        if let Err(err) = &result {
            samples.lock().unwrap_or_else(|e| e.into_inner()).error = Some(Status::Failed {
                os: err.raw_os_error(),
            });
        }
        Self {
            target,
            samples,
            cancel,
            worker: result.ok(),
        }
    }

    /// 現在値。エラー終了後も最後の観測値を返す (`status` が終端状態になる)。
    pub fn snapshot(&self) -> Snapshot {
        self.samples
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .snapshot(now_us())
    }

    /// 計測を止める。静止中にブロッキング read で待たないよう、
    /// ワーカーの同期 I/O を解除してから join する (通常は数 ms で戻る)。
    pub fn stop(&mut self) {
        self.cancel.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            #[cfg(windows)]
            while !worker.is_finished() {
                // 静止中のマウスは ReadFile を無期限にブロックする。stop 確認と
                // 次の read の間の競合も吸収するため、終わるまで繰り返し解除する。
                use std::os::windows::io::AsRawHandle;
                unsafe {
                    windows_sys::Win32::System::IO::CancelSynchronousIo(worker.as_raw_handle());
                }
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            let _ = worker.join();
        }
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 1 レコードを pcap ストリームへ追加するテスト用ビルダ。
    struct Pcap(Vec<u8>);

    impl Pcap {
        fn new() -> Self {
            let mut stream = vec![0; 24];
            stream[..8].copy_from_slice(&[0xd4, 0xc3, 0xb2, 0xa1, 2, 0, 4, 0]);
            stream[20] = 249;
            Self(stream)
        }

        #[expect(clippy::too_many_arguments)]
        fn record(
            &mut self,
            sec: u32,
            us: u32,
            dev: u8,
            ep: u8,
            info: u8,
            transfer: u8,
            status: u8,
            len: u8,
            report: u8,
        ) -> &mut Self {
            let mut header = [0u8; 16];
            header[..4].copy_from_slice(&sec.to_le_bytes());
            header[4..8].copy_from_slice(&us.to_le_bytes());
            header[8] = 28;
            header[12] = 28;
            let mut p = [0; 28];
            p[0] = 27; // header_len (interrupt IN ヘッダ 27B → payload は 27 から)
            p[10] = status;
            p[16] = info;
            p[19] = dev;
            p[21] = ep;
            p[22] = transfer;
            p[23] = len;
            p[27] = report;
            self.0.extend(header);
            self.0.extend(p);
            self
        }

        /// 任意サイズのレコード (ペイロードはゼロ埋め)。
        fn raw(&mut self, sec: u32, us: u32, size: u32) -> &mut Self {
            let mut header = [0u8; 16];
            header[..4].copy_from_slice(&sec.to_le_bytes());
            header[4..8].copy_from_slice(&us.to_le_bytes());
            header[8..12].copy_from_slice(&size.to_le_bytes());
            header[12..16].copy_from_slice(&size.to_le_bytes());
            self.0.extend(header);
            self.0.extend(vec![0; size as usize]);
            self
        }

        /// 対象の interrupt IN 成功レコードを `count` 件、`interval_us` 間隔で。
        fn motion(&mut self, sec: u32, count: u32, interval_us: u32) -> &mut Self {
            for i in 0..count {
                self.record(sec, i * interval_us, 5, 129, 1, 1, 0, 1, 0);
            }
            self
        }

        fn run(&self, target: Target) -> Samples {
            let samples = Mutex::new(Samples::default());
            let _ = consume(&self.0[..], target, &AtomicBool::new(false), &samples);
            samples.into_inner().unwrap()
        }
    }

    const TARGET: Target = Target {
        hub: 1,
        device: 5,
        endpoint: 129,
        report: None,
    };

    #[test]
    fn counts_only_target_interrupt_in_completions_with_data() {
        let mut s = Pcap::new();
        s.motion(10, 3, 1000)
            // 別アドレス・別 EP・要求側 (info bit0=0)・非 interrupt・失敗・空データは除外。
            .record(10, 3000, 6, 129, 1, 1, 0, 1, 0)
            .record(10, 3001, 5, 130, 1, 1, 0, 1, 0)
            .record(10, 3002, 5, 129, 0, 1, 0, 1, 0)
            .record(10, 3003, 5, 129, 1, 3, 0, 1, 0)
            .record(10, 3004, 5, 129, 1, 1, 1, 1, 0)
            .record(10, 3005, 5, 129, 1, 1, 0, 0, 0);
        let mut samples = s.run(TARGET);
        let snap = samples.snapshot(10_500_000);
        assert_eq!(snap.status, Status::Live);
        assert_eq!(snap.hz, 3);
        assert_eq!(snap.peak_hz, 3);
    }

    #[test]
    fn oversized_and_short_records_are_skipped_not_fatal() {
        // ハブ上の他デバイスが大きな転送 (64B report 等) や短い制御レコードを
        // 出しても計測は死なない — 以前はバッファ外のレコードで Failed 終了
        // していた (別アプリ前面化で procmon の HID 書込みが混ざると停止した)。
        let mut s = Pcap::new();
        s.record(10, 0, 5, 129, 1, 1, 0, 1, 0)
            .raw(10, 100, 91) // 64B バッファを超える転送
            .raw(10, 200, 10) // ヘッダより短いレコード
            .raw(10, 300, 0) // 空
            .record(10, 400, 5, 129, 1, 1, 0, 1, 0)
            .record(10, 500, 5, 129, 1, 1, 0, 1, 0);
        let samples = Mutex::new(Samples::default());
        // 全レコードを読み切ってストリーム末尾の UnexpectedEof で終わる。
        // サイズ外レコードで失敗すると InvalidData に、途中で止まると hz が減る。
        let err = consume(&s.0[..], TARGET, &AtomicBool::new(false), &samples).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::UnexpectedEof);
        let mut samples = samples.into_inner().unwrap();
        let snap = samples.snapshot(10_500_000);
        assert_eq!(snap.status, Status::Live);
        assert_eq!(snap.hz, 3);
    }

    #[test]
    fn report_id_filter_selects_matching_records() {
        let target = Target {
            report: Some(2),
            ..TARGET
        };
        let mut s = Pcap::new();
        s.record(10, 0, 5, 129, 1, 1, 0, 1, 2)
            .record(10, 1, 5, 129, 1, 1, 0, 1, 3)
            .record(10, 2, 5, 129, 1, 1, 0, 1, 0)
            .record(10, 3, 5, 129, 1, 1, 0, 1, 2);
        let mut samples = s.run(target);
        assert_eq!(samples.snapshot(10_500_000).hz, 2);
    }

    #[test]
    fn simulated_rates_fill_one_second_window() {
        for (rate, interval_us) in [(125u32, 8_000u32), (1000, 1000), (8000, 125)] {
            let mut s = Pcap::new();
            s.motion(10, rate, interval_us);
            let mut samples = s.run(TARGET);
            let snap = samples.snapshot(11_000_000);
            // (t-1s, t] なので最後の 1 秒に収まるのは rate - 1 件。
            assert_eq!(snap.hz, rate - 1, "rate {rate}");
            // ピークは各受信時点の窓 — 区間全体 (~1秒) が収まるので rate 件。
            assert_eq!(snap.peak_hz, rate);
        }
    }

    #[test]
    fn one_second_boundary_excludes_older_records() {
        let mut s = Pcap::new();
        s.record(10, 0, 5, 129, 1, 1, 0, 1, 0) // t = 10.000000 (境界ちょうど外側)
            .record(10, 1, 5, 129, 1, 1, 0, 1, 0); // t = 10.000001
        let mut samples = s.run(TARGET);
        assert_eq!(samples.snapshot(10_500_000).hz, 2);
        // now = 11.000000 → 窓は (10.000000, 11.000000]。10.000000 丁度は含まない。
        assert_eq!(samples.snapshot(11_000_000).hz, 1);
        // 全て期限切れになれば静止中の 0 Hz。
        assert_eq!(samples.snapshot(12_000_000).hz, 0);
    }

    #[test]
    fn peak_keeps_densest_window_during_run() {
        let mut s = Pcap::new();
        // 低速区間 → 1 秒の密な区間 (8000 件) → 孤立 1 件。
        s.motion(10, 10, 100_000);
        for i in 0..8000 {
            s.record(11, i * 125, 5, 129, 1, 1, 0, 1, 0);
        }
        s.record(12, 500_000, 5, 129, 1, 1, 0, 1, 0);
        let mut samples = s.run(TARGET);
        // now = 13.0004 → 直近 1 秒には 12.5 の 1 件だけだが、ピークは残る。
        let snap = samples.snapshot(13_000_400);
        assert_eq!(snap.peak_hz, 8000);
        assert_eq!(snap.hz, 1);
        assert_eq!(snap.status, Status::Live);
    }

    #[test]
    fn short_record_and_truncated_stream_are_errors() {
        // サイズがヘッダ (27B) 未満のレコードは対象外として捨てる — 読み切りで EOF。
        let mut s = Pcap::new();
        s.0.extend([0u8; 16]); // incl_len = 0
        let samples = Mutex::new(Samples::default());
        let err = consume(&s.0[..], TARGET, &AtomicBool::new(false), &samples).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::UnexpectedEof);
        // 途中 EOF → UnexpectedEof。
        let mut t = Pcap::new();
        t.motion(10, 2, 1000);
        t.0.truncate(t.0.len() - 7);
        let err = consume(
            &t.0[..],
            TARGET,
            &AtomicBool::new(false),
            &Mutex::new(Samples::default()),
        )
        .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::UnexpectedEof);
        // グローバルヘッダが違うものも不正。
        let err = consume(
            &[0u8; 24][..],
            TARGET,
            &AtomicBool::new(false),
            &Mutex::new(Samples::default()),
        )
        .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn status_maps_os_errors_by_stage() {
        let e = |code: i32| io::Error::from_raw_os_error(code);
        assert_eq!(
            status_of(&e(2), Stage::Open),
            Status::DriverMissing { os: Some(2) }
        );
        assert_eq!(
            status_of(&e(5), Stage::Open),
            Status::AccessDenied { os: Some(5) }
        );
        assert_eq!(
            status_of(&e(32), Stage::Open),
            Status::HubBusy { os: Some(32) }
        );
        assert_eq!(
            status_of(&e(1167), Stage::Run),
            Status::Disconnected { os: Some(1167) }
        );
        assert_eq!(
            status_of(
                &io::Error::new(io::ErrorKind::UnexpectedEof, "x"),
                Stage::Run
            ),
            Status::Disconnected { os: None }
        );
        assert_eq!(
            status_of(&e(1167), Stage::Open),
            Status::Failed { os: Some(1167) }
        );
        assert_eq!(
            status_of(&e(87), Stage::Run),
            Status::Failed { os: Some(87) }
        );
    }

    #[test]
    fn waiting_until_first_record_then_zero_when_idle() {
        // 初回受信前は Waiting。
        let mut fresh = Samples::default();
        assert_eq!(fresh.snapshot(0).status, Status::Waiting);
        // 受信後に 1 秒以上空けば Live のまま 0 Hz。
        let mut s = Pcap::new();
        s.motion(10, 1, 1000);
        let mut samples = s.run(TARGET);
        let snap = samples.snapshot(12_500_000);
        assert_eq!(snap.status, Status::Live);
        assert_eq!(snap.hz, 0);
        // エラーは終端状態として status に残る。
        samples.error = Some(Status::HubBusy { os: Some(32) });
        assert_eq!(samples.snapshot(0).status, Status::HubBusy { os: Some(32) });
    }
}
