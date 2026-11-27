---
paths:
  - tools/bench
  - Makefile
  - app/service/cmd/unicd/bench_probe.go
  - app/desktop/cmd/unic-desktop/bench_probe.go
  - app/desktop/internal/desktopui/bench_probe.go
  - app/desktop/internal/desktopui/bench_probe_noop.go
  - .github/workflows/desktop-benchmark.yml
verified: b1b777fdf83d7eb54aebf56671ab7f586c011136
---
# Win/Mac 桌面资源 Benchmark

仓库在 `tools/bench/` 提供独立、可复跑的双平台（macOS 与 Windows）桌面资源基准测量与回归对比工具链（`unic-bench`）及 Parallels Windows 11 VM 编排脚本，用于量化后台常驻、前台空闲、前台交互、关闭回收、最小化留存与独立 Quick Panel 生命周期的内存、CPU/唤醒、句柄/FD/GDI/USER 与交互就绪时延基线。本工具链仅负责可信度量与归因，不包含任何生产性能优化代码。

## 现状

- **四进程桌面拓扑与轻量对比**：
  - 正常桌面形态最多同时存在四个角色进程：`unicd`（无头后台服务）、`tray`（系统托盘宿主与适配器）、`gui`（`unic-desktop` 主窗口，关闭窗口后进程退出）和 `quick-panel`（`unic-desktop --quick-panel` 独立预热/常驻进程）。
  - 轻量无头模式（`unic daemon start`）仅运行单进程 `unicd`；通过 `bg-idle-normal (standard) - bg-idle-lightweight (standard)` 精确量化 `tray + standby quick-panel` 常驻形态相对纯后端服务的净增量开销。
- **双轨构建边界（Track A vs Track B）**：
  - **Track A（`production`）**：无任何额外构建 tag、无 `-race`、使用 `-trimpath -ldflags="-s -w"`（Windows 桌面端附加 `-H windowsgui`），绑定真实 OS `nativeboard` 剪贴板实现；零 `automation.sock`、零 `bench_probe` 代码、零额外 goroutine 与端口，用于真实交付体量评估。
  - **Track B（`interactive`）**：仅用于交互与深度缓存归因。`unicd` 使用 `-tags bench_probe`，`unic-desktop` 使用 `-tags desktop_e2e,bench_probe`；启用 `UNIC_DESKTOP_E2E_STATE_DIR` 隔离剪贴板状态、`UNIC_DESKTOP_E2E_AUTOMATION_SOCKET`（`automation.sock` UI 自动化驱动）以及仅在 `UNIC_BENCH_METRICS_DIR` 非空时激活的 5s JSONL 诊断探针。
  - Track B `bench_probe` 采集 `runtime/metrics`（`heap_alloc`、`heap_inuse`、`heap_sys`、`heap_released`、`stack_inuse`、`num_gc`、`goroutines`）以及桌面 UI 缓存与 `weak.Pointer` 对象存活归因（条目数、detail 缓存、image owner、原图、GPU 纹理存活）。探针严禁监听端口、记录剪贴板/历史正文或调用 `runtime.GC()` / 覆盖 `GOGC` / `GOMEMLIMIT`。
- **双平台指标物理口径与事后归因（不可混算）**：
  - **macOS**：headline 指标采用内核 `ri_phys_footprint`（`PhysFootprintBytes`，对齐 Activity Monitor 内存列），同步采样 `RSSBytes`、`LifetimeMaxPhysFootprintBytes`、`cpu_total_ns`、idle/interrupt wakeups、threads、FDs；阶段末采集 `footprint -j` 区域归因与 `top` 交叉校验。
  - **Windows**：headline 指标采用进程独占工作集 `PrivateWorkingSetSize`（`PrivateWorkingSetBytes`，对齐 Task Manager 内存列），同步采样独占提交 `PrivateUsage`（`PrivateCommitBytes`）、总工作集 `WorkingSetSize`（含跨进程共享 DLL/系统页计费，显式标注共享页说明）、句柄数 `Handles`、GDI 对象、USER 对象与可见/前台窗口数；阶段末通过 `VirtualQueryEx` 区分 `PrivateCommit` 与 `Mapped/ImageCommit`（明确 `VirtualQueryEx` 反映虚拟地址提交，不等同物理原生堆或 GPU 显存驻留），并通过 `debug/pe` 与 Mach-O 头部权威识别二进制架构与仿真状态（`emulated=true/false`）。
- **六大核心场景、数据集与收敛/时延模型**：
  - `bg-idle-normal`（`empty`、`standard`）、`bg-idle-lightweight`（`standard`）、`fg-visible-idle`（`empty`、`standard`）、`fg-interactive`（Track B `standard`，含滚动/搜索/选中/原图预览交互与 15s静置）、`fg-close-reclaim`（`standard`，`pre-open-bg` → 5 轮开关 GUI 回收与线性回归泄漏斜率检查 → `post-close-bg` 与 `pre-hide-visible` → `gui-hidden-retained` 最小化/隐藏对比）、`qp-lifecycle`（`standard` Track A & Track B：冷启 → standby → 10/20 轮热开关回收、斜率与句柄/FD/GDI/USER 漂移）。
  - 确定性合成语料固定种子 `20261120`：`empty`（0 条）、`standard`（200 条文本 + 12 张 `2048x1536` PNG + 5 个文件）、`soak`（1000 条文本 + 30 张 `2048x1536` PNG + 10 个文件）；定时配置分 `default`（后台 45s 预热/60s 稳态、前台 10s/30s、QP 热开关 10 轮）与 `soak`（后台 45s/300s、QP 热开关 20 轮），采样周期均为 1s。
  - 针对 `unicd` 冷启动 Go runtime 约 ~30–35s 后回落的特性，编排器在基线/回收场景前等待 `unicd` 渡过启动峰值并收敛，将启动瞬间峰单独记为 `EarlyPeak` 而不污染稳态中位数与 `reclaim_ratio_pct`。
  - 时延拆解记录 `ipc_to_visible_ms`、`ipc_to_search_ready_ms`、`ipc_to_original_ready_ms` 等进程内 IPC/自动化派发至渲染/就绪状态耗时，不声称包含 OS 全局热键驱动或显示器首帧光子延迟。

## 不变量

- **生产构建零污染**：默认 `unicd` 与 `unic-desktop` 构建（无 `bench_probe`、`desktop_e2e` tag）绝不包含探针代码、不开启 `automation.sock`、不产生额外 goroutine；跨 OS 编译与 `production-boundary` 测试守住 `desktopdemo` 与核心 `engine/`/`internal/` 无依赖回流。
- **全量隔离与剪贴板无损（F1–F12）**：所有运行使用独立临时 `UNIC_HOME` 并强制 `Sync.Enabled=false`；禁止触碰或杀掉用户常规剪贴板或既有非本轮创建的 `unic*` 进程（仅按精确 PID 生命周期清理）；Track A 与 Track B 前后校验系统剪贴板序列号不变（`clipboard_unchanged=true`）。
- **Parallels VM 状态自动还原与产物核验**：`tools/bench/vm/run_parallels.py` 通过 `prlctl` 探测 VM 初始状态（如 `suspended`/`stopped`/`running`），仅当脚本主动恢复/启动了 VM 时才在结束/异常退出后恢复原状，避免干扰独立任务；宿主机交叉编译的 Track A/B 二进制须经 SHA-256 与 PE 架构校验后拷贝至 Guest 本地路径运行，最终报告输出至 `.build/bench/` 下的 Git 忽略目录。
- **严谨比较口径**：禁止跨越不同构建轨、不同数据集、不同样本平台或仿真态直接宣布绝对内存回归；`unic-bench compare` 仅对同平台/同配置基线对比相对变化与 `LeakCheck` 斜率阈值，且不在同一基准任务中混入生产代码优化改动。
- **零外链离线 HTML 可视化看板**：`unic-bench run` 同步生成同目录单文件离线 `report.html` 并纳入 `SHA256SUMS`（`tools/bench/vm/run_parallels.py` 同步回收）；`unic-bench render`（及 `make bench-html` / CI `dashboard` 作业）支持单报告与多平台并列看板渲染及 `--compare`/`--budget` 回归可视化，采用纯 Go 标准库 `html/template` + `embed` 内联 SVG/JS 与严格 CSP（`default-src 'none'`），守住双平台口径隔离、`VirtualQueryEx` `ReservedBytes` 独立展示、安全 JSON 转义与纯 DOM 文本渲染。

## 代码地图

| 路径 | 职责 |
|---|---|
| `tools/bench/cmd/unic-bench/` | CLI 入口，提供 `doctor`、`seed`、`run`、`compare`、`render` 五个子命令 |
| `tools/bench/internal/fixture/` | 确定性合成语料生成（`empty`/`standard`/`soak`）、临时 `UNIC_HOME` 隔离与 PID 级 daemon 管理 |
| `tools/bench/internal/sampler/` | macOS（`top`/`footprint`）与 Windows（`WorkingSet`/`PrivateWorkingSet`/`PrivateCommit`/`VirtualQueryEx`/Handles/GDI/USER/`debug/pe`）原生采样器及回归统计 |
| `tools/bench/internal/scenarios/` | 6 大场景状态机编排、`unicd` 启动收敛等待、Track B 自动化驱动与时延/缓存归因聚合 |
| `tools/bench/internal/report/` | 产出 `report.json`、`report.md`、零外链单文件离线交互看板 `report.html`、`comparison.json`/`comparison.md` 与 `SHA256SUMS`（`render` 支持单/多报告同屏对比及 `--compare`/`--budget` 可视化，严格守住双平台口径隔离与 XSS 转义） |
| `tools/bench/vm/run_parallels.py`、`test_run_parallels.py` | 宿主机交叉编译（`windows/arm64` 或 `amd64`）、Parallels VM 生命周期管理、Guest 隔离执行、产物回收（含 `report.html`）与单测 |
| `app/service/cmd/unicd/bench_probe.go` | `//go:build bench_probe` 纯标准库 `runtime/metrics` JSONL 探针（零 UI 依赖） |
| `app/desktop/cmd/unic-desktop/bench_probe.go`、`app/desktop/internal/desktopui/bench_probe*.go` | `//go:build bench_probe` 桌面 UI `runtime/metrics` + `weak.Pointer` 缓存归因与无探针 noop 桩 |
| `Makefile`、`.github/workflows/desktop-benchmark.yml` | 本地 `bench-mac` / `bench-win-vm` / `bench-compare` / `bench-html` 入口与纯手动（`workflow_dispatch`）跨端 CI Workflow（含汇总离线看板 `dashboard` 作业） |

## 规格与背景

- 首轮双平台实测基线回执：[Desktop 资源 Benchmark 首轮双平台基线回执](../exec/desktop-resource-benchmark-baseline.md)（macOS ARM64 + Windows 11 ARM64 VM 全量 `9/9 PASS`、归因结论与校验指纹）。
- 桌面运行时与组件契约：[桌面运行时与独立进程](desktop-runtime.md)、[Desktop E2E 契约](../contracts/desktop-e2e-contract.md)。
- 历史专项研究入口（已标记由本工具链取代）：[desktop-image-memory](../../tools/research/desktop-image-memory/README.md)、[quick-panel-memory](../../tools/research/quick-panel-memory/README.md)、[windows-desktop-memory](../../tools/research/windows-desktop-memory/README.md)。

## 缺口

- 物理 `windows/amd64` 与物理 Intel (`darwin/amd64`) 主机基线尚未实机采集（`NOT_RUN`）；`.github/workflows/desktop-benchmark.yml` 已配置 `windows-2025` 与 `macos-15-intel` 手动矩阵待调度验证。
- 物理全局热键与显示器首帧光子延迟、24 小时睡眠/唤醒及开启远端同步（`sync.enabled=true`）下的长周期网络资源基准仍不在当前场景覆盖范围内（`NOT_RUN`）。
