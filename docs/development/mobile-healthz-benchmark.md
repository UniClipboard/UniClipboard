# Mobile LAN `/healthz` 性能基准

本文定义 `/healthz` 与 `/SyncClipboard.json` 性能证据的可重复运行方式。两个基准用途不同，不能互相替代。

## 快速回归门禁

`crates/uc-webserver/src/mobile_lan/healthz_load.rs` 在同一进程内启动真实 TCP listener 和 load generator，并让控制组使用生产参数的 Argon2id。它用于快速发现 `/healthz` 重新进入鉴权或业务依赖等结构性回归。

```bash
cargo test -p uc-webserver --release --lib -- \
  --ignored --nocapture healthz_load_vs_sync_clipboard_json
```

该门禁会在计算和输出比率前强制要求：

- 两个路径的错误数均为 0。
- 两个路径均至少产生一个成功样本。

它报告的是 listener 与 load generator 所在进程的平均 CPU，不是 daemon PID 的峰值 CPU，也不读取真实 daemon 日志。因此这组结果不能单独作为 PRD 性能证据。

## 进程隔离基准

`mobile_healthz_process_bench` 会执行以下完整流程：

1. 启动独立 `uniclipd` profile，并从 `Child::id()` 获取被采样 PID。
   benchmark 强制使用 portable 文件安全存储，结束后删除该 profile 的 data/cache，
   不在系统 Keychain、Credential Manager 或 Secret Service 遗留 KEK。profile
   只允许 ASCII 字母、数字、`-`、`_`，且 daemon/CLI 必须与 benchmark
   二进制同目录；清理前还会校验目标路径与该 portable profile 精确相等。
2. 通过 `uniclip init` 初始化加密 Space。
3. 通过非交互 `uniclip --json mobile setup` 启用 mobile LAN listener，
   注册使用生产 Argon2id 的 Basic Auth 设备。
4. 等待默认 5 秒启动稳定期，避免把 iroh 首次 relay 建连等一次性后台事件
   归因给请求路径。
5. 从独立 load generator 进程以 20 并发分别持续请求两个路径 30 秒。
6. 每 200 ms 采样一次 daemon PID CPU，记录观测峰值；`100%` 表示占满
   一个逻辑核心。
7. 读取 `UC_LOG_FILE` 指定的真实 daemon JSON 日志，比较每个路径运行
   前后的全进程 INFO/WARN 行数，以及 `uc_webserver::mobile_lan` 请求路径
   INFO/WARN 行数。
8. 输出机器型号、操作系统、架构、逻辑核心数、请求数、错误数、
   p50/p95/p99、daemon 峰值 CPU 和两套日志增量。请求总数精确计数；
   延迟写入固定内存、3 位有效数字的 HDR Histogram，避免高吞吐 `/healthz`
   因保存数百万个原始样本而让 30 秒基准失去有界内存行为。

先分别构建三个 release 二进制，避免 Cargo 多包目标选择掩盖缺失产物：

```bash
cargo build --release -p uc-daemon --bin uniclipd
cargo build --release -p uc-cli --bin uniclip
cargo build --release -p p2p-bench --bin mobile_healthz_process_bench
```

运行时必须提供绝对日志路径：

```bash
UC_LOG_FILE="$(pwd)/target/mobile-healthz-daemon.jsonl" \
  target/release/mobile_healthz_process_bench
```

`UC_LOG_FILE` 是 daemon 诊断专用的精确文件覆盖；同时设置该变量的进程
若 `UC_HOST_ROLE` 不是 `daemon` 会直接失败，避免 GUI、CLI 与 daemon 并发
写同一文件。daemon 未设置该变量时仍使用现有的按角色、按日滚动日志。
精确文件使用无损队列，基准会先清空并验证它可读写；每个 arm 结束后必须
连续读到稳定的文件长度和计数才生成快照。路径缺失、相对路径、权限错误、
非法 UTF-8、非 JSON 行、不完整尾行或在截止时间内未稳定都会直接令运行
失败，不能折算成零日志增量。

## 验收条件

一次可引用的完整运行必须同时满足：

- 配置为 20 并发、每路径 30 秒。
- 两个路径均有成功样本且错误数为 0。
- CPU 采样始终能读取同一个 daemon PID，并至少产生一个样本。
- `/healthz` 的真实 `uc_webserver::mobile_lan` 日志增量为 `INFO +0, WARN +0`。
- `/SyncClipboard.json` 在真实 `uc_webserver::mobile_lan` 日志中至少产生一条 INFO，且不产生 WARN。
- 全进程 INFO/WARN 增量必须原样输出；daemon 后台任务产生的日志不能被静默折算为端点日志，也不能导致日志文件不可读时误报为零。
- 结果包含 p50、p95、p99；结果表不得省略 p95。

机器负载、调度器和采样间隔都会影响峰值 CPU，因此不要把普通共享 CI 上的一次峰值设为硬阈值。性能阈值应在固定硬件的专用运行中判定；请求正确性与日志不变量则由每次基准硬性验证。

## 烟雾验证

开发中可缩短时长来验证进程、认证、日志和采样闭环，但该结果不可引用为 PRD 证据：

```bash
UC_LOG_FILE="$(pwd)/target/mobile-healthz-daemon-smoke.jsonl" \
  target/debug/mobile_healthz_process_bench \
  --daemon-bin target/debug/uniclipd \
  --cli-bin target/debug/uniclip \
  --concurrency 2 \
  --duration-secs 1
```
