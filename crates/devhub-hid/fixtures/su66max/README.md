# SU66 Max フィクスチャ

`probe1.txt` — 2026-11-15 に実機 (VID:PID `1ca6:3009`, usagePage `FFB0`) へ各コマンドを
送った全応答の 16 進ダンプ。`.req.bin` / `.resp.bin` はその一部を 64B バイナリ化したもの。

| name | 要求 | 応答の要点 |
|---|---|---|
| `perf` | `[4,1,1,0]` 性能読み (row1,col0) | mode=0, nP=nR=2.0, rtF=0.5, rtP=rtR=0.3, dZ=0.2/0.2, axis=0, calib=1, axisV2=0x3561, coef=0x0c80, rangeMax=0.1 |
| `higher_none` | `[6,1,1,0,0]` 高度キー読み | mode=0 (NONE、未設定) |
| `macro_space` | `[2,14,0]` マクロ領域 | r[3]=16 スロット, r[4,5]=960 最大イベント数 |
| `poll_read` | `[2,5,1]` ポーリング現在値 | r[3]=0 (8KHz) |
| `route_r1` | `[4,3,1,1]` route 行1 | 全 0 (無押下)。`live_travel` はこれを行 1..5 で収集 |

注: probe 時点では全キー mode=0 (通常)・高度キー未設定・マクロ空。
`axisRangeMax` (perf r[25,26]) は 0.1mm を返した — 全ストロークではないので
`XsydInfo::travel` は既定 4.0mm を使う (0.5mm 未満の値は採用しない)。
