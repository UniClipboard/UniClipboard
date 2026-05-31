# ADR-008 P0 Perf Spike 结果：blob 过 loopback（OQ-perf-gate）

- **目的**：回答 [ADR-008](./adr-008-uniclipd-split-gui-as-client.md) 的 **OQ-perf-gate / D6**——GUI 拆进程后大负载经 `127.0.0.1` HTTP 按需拉取，loopback 链路够不够快、现有 **全量 buffer**（`BlobReaderPort::get → Vec<u8> → Body::from = Full<Bytes>`）会不会拖垮 daemon 内存。
- **日期**：2026-05-30
- **机器**：Apple M4 / 10 core / 24 GB（macOS darwin）。loopback 吞吐为 **上界**（无真实网络拥塞）。
- **bench**：`src-tauri/crates/p2p-bench/src/bin/http_blob_bench.rs`（throwaway，`publish=false`，iroh 依赖已 feature-gate 不牵连）。最小 axum server 忠实复刻生产 full-buffer blob 端点 + 一个对照 streaming 变体（从磁盘分块读，模拟流式 `BlobReaderPort`）。
- **复现**：
  ```
  cd src-tauri && cargo build --release -p p2p-bench --bin http_blob_bench
  /usr/bin/time -l target/release/http_blob_bench --payload-bytes 67108864 --concurrent 4 --rounds 20 --warmup 3 --mode full-buffer
  ```

## 1. 忠实度

- **忠实**：内存足迹（`Vec<u8>` 分配 = 整块 payload）、传输形态（`Full<Bytes>` 全量 buffer、无 streaming/Range/chunked/压缩，与生产一致——`uc-webserver/src/api/blob.rs`、`uc-infra/.../filesystem_store.rs` 已核实）。端口签名 `get → Vec<u8>` 强制全量，生产 **不可能** 不 buffer。
- **不忠实（已知、不静默补）**：bench 省了生产的 auth + rate-limit 中间件（~10–100µs，≥1MiB 时为噪声）与真实磁盘 I/O；故 bench 的 TTFB 是 **下界**。streaming 变体是 **假设性** 的（端口改流式后才存在），仅用于量化"改流式能回收多少"。
- bench 在 `concurrent=1` 同时持"共享模板 blob(N) + 单请求 clone(N)"≈2N；生产无共享模板，**每个在途请求 ≈ 1×N 常驻**。下文按生产等价口径解读（K 并发 ≈ K×N + baseline）。

## 2. 数据

### 2.1 full-buffer 单连接扫负载

| payload | TTFB p50/p95/p99 (ms) | 吞吐聚合 (GiB/s) | max RSS |
|---|---|---|---|
| 64 KiB | 0.21 / 0.27 / 0.29 | 0.33 | 3.7 MiB |
| 1 MiB | 0.61 / 0.75 / 0.85 | 1.30 | 8.1 MiB |
| 8 MiB | 0.86 / 1.13 / 1.15 | 4.14 | 20.9 MiB |
| 64 MiB | 2.29 / 2.65 / 2.79 | 8.78 | 133.5 MiB |
| 256 MiB | 7.65 / 9.87 / 11.64 | 7.82 | 517.6 MiB |

### 2.2 full-buffer 并发=4（P0 场景）

| payload×并发 | TTFB p50/p95/p99 (ms) | 吞吐聚合 (GiB/s) | max RSS |
|---|---|---|---|
| 64 MiB ×4 | 4.29 / 7.37 / 9.62 | 11.9 | **328 MiB** |
| 256 MiB ×4 | 18.9 / **155** / **162** | 10.7 | **1287 MiB** |

### 2.3 streaming 对照（假设端口改流式，从磁盘分块读）

| 场景 | TTFB p50/p99 (ms) | 吞吐聚合 (GiB/s) | max RSS |
|---|---|---|---|
| 任意档 ×1（64KiB→256MiB） | ~0.10 / ~0.15（**恒定**） | ~0.35 | **~6 MiB（恒定）** |
| 64 MiB ×4 | 0.22 / 0.47 | 1.07 | **9.3 MiB** |
| 256 MiB ×4 | 0.21 / 0.40 | 1.03 | **10.1 MiB** |

## 3. 结论

1. **loopback 传输不是瓶颈。** full-buffer TTFB 即便 256MiB 仍 <10ms（单连接）、吞吐 8–12 GiB/s。原拟门槛（TTFB 缩略图 <50ms / 1MiB <150ms / 8MiB <400ms；吞吐 ≥500MiB/s）被 **超出 1–2 个数量级**。结论支撑 D4"复用 HTTP loopback、不为延迟另起 UDS/共享内存"。

2. **瓶颈是 full-buffer 内存 × 并发。** RSS ≈ baseline + 在途请求数 × N。P0 64MiB×4 = 328MiB；256MiB×4 = **1.29 GiB**。比值口径 full-buffer **通过** 原拟门槛（64MiB×4:328/256=1.28×<1.5×；256MiB×4:1287/1024=1.26×<1.5×——无超出 clone 数的隐性放大），但 **绝对值** 对一个后台常驻 daemon 是真实风险。

3. **并发 + 大负载还会击穿尾延迟。** 256MiB×4 的 p99=162ms（对比 64MiB×4 的 9.6ms）——大 `Vec` 在 4 路并发下的分配/释放压力让尾延迟暴涨，恰好在内存吃紧时"TTFB 还好"的结论失效。

4. **改流式能回收几乎全部。** streaming 下 RSS **恒定 ~6–10MiB**（与 payload/并发无关，省 35–127×）、TTFB **恒定 ~0.2ms**（无需先物化整块，256MiB×4 尾延迟好 ~400×），代价是吞吐降到 ~0.35–1.1 GiB/s（仍 >500MiB/s 门槛；且此为 `ReaderStream` 默认小 chunk 的 **地板值**，调大 chunk 可显著提升）。

## 4. 门槛裁定（回填 OQ-perf-gate / D6）

- **保留 full-buffer 服务常见档**：缩略图 + 图片 ≤ 内联阈值时，RSS 与 TTFB 都无忧（8MiB：单连接 21MiB / <1ms）。
- **自动内联预览阈值 = 8 MiB（D6 钦定）**：WS/预览路径对 ≤8MiB 走现有 full-buffer blob 端点内联；**>8MiB 不自动预览**，转显式用户发起的下载。
- **大文件显式下载路径优先改流式 `BlobReaderPort`**（streaming 数据证明 RSS/TTFB 双双拍平）；在改流式之前，**对 >内联阈值的 full-buffer 并发拉取加并发上限（信号量）**，把最坏 daemon RSS 钉死（每路 ~N）。
- **内存硬门（实测校准）**：单条 full-buffer 拉取 RSS 增量 ≈ 1×payload（生产口径）；并发 K 路 ≈ K×payload + baseline，无隐性放大。门槛设"并发 N 路 full-buffer RSS 增量 < (N + 0.5)×payload"即可，本实测满足；真正的运营约束是 **绝对上限**——故用内联阈值 + 并发信号量把 >8MiB 路径的常驻封顶。

## 5. 残余风险 / 未测

- TTFB 为下界（省了 auth + rate-limit 中间件与真实磁盘读）；绝对延迟门槛已有 1–2 数量级余量，不影响裁定。
- loopback 吞吐是上界（无真实网络拥塞）；按需拉的瓶颈本就是内存非吞吐，不影响裁定。
- streaming 吞吐为 `ReaderStream` 默认 chunk 的地板值；若落地改流式，应实测调优 chunk 大小（256KiB–1MiB）。
- `BlobReaderPort` 是否支持流式读未在本 spike 验证（退路：维持 buffer + 用内联阈值 + 并发上限挡超大 payload）。
