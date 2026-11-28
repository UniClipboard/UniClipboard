# Desktop 资源 Benchmark 首轮双平台基线回执

本回执汇总首轮隔离可控的 macOS ARM64 与 Windows 11 ARM64（Parallels VM）桌面资源基线实测结果，以及对应的客观归因观测与未测边界。所有结论基于本次实测产物，不代表长期优化目标；后续性能优化将基于本基线独立立项，本阶段未修改生产优化路径。

> **口径声明**：macOS 主内存指标为内核 `phys_footprint`（`top` + `footprint -j`）；Windows 主内存指标为进程独占 `PrivateWorkingSet`（辅以 `PrivateCommit`、`WorkingSet` 含共享 DLL 计费与 `VirtualQueryEx` 区域提交，以及句柄 / GDI / USER 对象数）。两平台内存物理定义不同，横向并列用于观察各场景增量与释放趋势，不可做绝对字节等价比较。

---

## 1. 环境、构建与产物指纹

| 维度 | macOS ARM64 (`mac-slice2-fix1`) | Windows 11 ARM64 VM (`win-slice3`) |
|---|---|---|
| OS / Host Arch | `darwin/arm64` (OS `27.0`) | `windows/arm64` (OS `10.0.26200`, Parallels VM) |
| 硬件规格 | Apple M4, 10 逻辑核, 24.00 GiB RAM | ARMv8 Family 8, 4 逻辑核, 5.99 GiB RAM |
| Go / 源码版本 | `go1.27.4` / `a3cb68a49626` (dirty=true) | 同左（宿主机 `CGO_ENABLED=0 GOOS=windows GOARCH=arm64` 交叉编译） |
| 二进制架构与仿真 | 全部 Mach-O `arm64`，`emulated=false` | 全部 PE `arm64`（`0xaa64`），`emulated=false` |
| 剪贴板完整性（F9） | sequence `487 -> 487` (`unchanged=true`) | sequence `703 -> 703` (`unchanged=true`) |
| Track A 构建标签 | 无额外 tag（无 `-race`，`-trimpath -ldflags="-s -w"`） | 同左（桌面端带 `-H windowsgui`） |
| Track B 构建标签 | `unicd`: `bench_probe`；`unic-desktop`: `desktop_e2e,bench_probe` | 同左（桌面端带 `-H windowsgui`） |

原始基线校验摘要（产物保存在已忽略目录 `.build/bench/mac-slice2-fix1/` 与 `.build/bench/win-slice3/`，不入 git）：

- **macOS `SHA256SUMS`**:
  - `report.json`: `dbd7982c4e2233a38a924e041b43ce0b1ca8e69c3f4e9072e6adcb82a825ab3d`
  - `report.md`: `85675c90d62fe4f6d02f6ca0892e71d24e40a5bb3b2f01b77ce9b6b81f5621b8`
  - 二进制 SHA-256 前缀：`unic=8754b3b5c432`, `unicd=06a28afea131`, `unic-desktop=700c22af3df5`, `unicd-bench-probe=0f36f856309d`, `unic-desktop-e2e-probe=ed09ab4e4087`
- **Windows 11 ARM64 `SHA256SUMS`**:
  - `report.json`: `09ac4a67cd576724365397d4c63c1890cf1e799d6c5307794f099dea6d6fe1fd`
  - `report.md`: `83ea9de987e72ad999ad3cec43e2c05c1d04c15302dd8fc86737a0bafea30011`
  - 二进制 SHA-256 前缀：`unic.exe=e8c55ed6129f`, `unicd.exe=d31e745c48c8`, `unic-desktop.exe=b2d80ef0d263`, `unicd-bench-probe.exe=1af8b628bebf`, `unic-desktop-e2e-probe.exe=e13b13c3fb37`, `unic-bench.exe=4f642cbb54dd`

---

## 2. 六大核心场景横向对比（6/6 场景、9/9 组合全部 `PASS`）

### 2.1 后台常驻（Track A，`empty` + `standard`，3 轮中位）

| 场景 / 数据集 | macOS (`phys_footprint` / CPU) | Windows (`PrivateWS` / `PrivateCommit` / CPU) | 关键构成（分角色中位） |
|---|---|---|---|
| `bg-idle-normal` (`empty`) | `71.69 MiB`（EarlyPeak `111.19 MiB`） / `0.23%` | `36.82 MiB` / `176.75 MiB` / `0.37%` | Win: `unicd 13.16`, `tray 12.98`, `quick-panel 10.68 MiB`<br>Mac: `unicd 24.47`, `tray 25.81`, `quick-panel 21.41 MiB` |
| `bg-idle-normal` (`standard`) | `70.63 MiB`（EarlyPeak `111.11 MiB`） / `0.37%` | `36.51 MiB` / `175.30 MiB` / `0.53%` | Win 句柄/资源：`H=461+295+202`, `GDI=0+10+7`, `USER=7+8+6`<br>Mac: `unicd 23.19`, `tray 25.89`, `quick-panel 21.55 MiB` |
| `bg-idle-lightweight` (`standard`) | `22.19 MiB`（EarlyPeak `64.55 MiB`） / `0.52%` | `12.75 MiB` / `58.92 MiB` / `0.32%` | 仅单进程 `unicd`（`tray`/`gui`/`quick-panel` 均不存在） |
| **`tray + standby quick-panel` 净增量** | **`+48.44 MiB`**（`50,792,728 B`, CPU `-0.15%`） | **`+23.75 MiB` PrivateWS**（`24,907,776 B`） / **`+116.38 MiB` PrivateCommit** | `bg-idle-normal (standard)` 减去 `bg-idle-lightweight (standard)` |

### 2.2 前台可见空闲（`fg-visible-idle`，Track A，3 轮中位）

| 数据集 | macOS (`phys_footprint` 稳态 / P95 / EarlyPeak / CPU) | Windows (`PrivateWS` 稳态 / `PrivateCommit` / EarlyPeak / CPU) | `ipc_to_visible_ms` (Mac vs Win) |
|---|---|---|---|
| `empty` | `114.90` / `115.18` / `176.49 MiB` / `0.63%`（`gui 41.35 MiB`） | `69.76` / `258.79` / `69.57 MiB` / `0.91%`（`gui 31.85 MiB`） | `280.5 ms` vs `486.4 ms` |
| `standard` | `115.80` / `116.16` / `177.25 MiB` / `0.34%`（`gui 41.58 MiB`） | `161.90` / `350.30` / `161.50 MiB` / `0.70%`（`gui 97.18 MiB`, `unicd 41.07 MiB`） | `235.4 ms` vs `729.9 ms` |

> **Windows 归因（`fg-visible-idle standard`）**：`VirtualQueryEx` 显示 `gui` 独占提交 `108.13 MiB` + 映射提交 `94.25 MiB`（含 DXGI/D3D11 纹理/共享缓冲）、`H=565`, `GDI=58`, `USER=27`；macOS `gui` 阶段末 `footprint=40.91 MiB` 含 `IOAccelerator=3.73 MiB`、`malloc=25.30 MiB`、`mach_ports=216`。

### 2.3 前台深度交互与探针归因（`fg-interactive`，Track B `standard`）

| 指标 | macOS (`darwin/arm64`) | Windows 11 (`windows/arm64`) |
|---|---|---|
| Active 稳态 / P95 / Peak / CPU | `218.85` / `244.37` / `263.04 MiB` / `21.38%` | `262.82` / `269.36` / `276.96 MiB` (`Commit 450.50 MiB`) / `22.52%` |
| Settle 15s 稳态 / P95 / CPU | `224.07` / `224.43 MiB` / `0.31%` (`gui 127.44 MiB`) | `278.35` / `302.44 MiB` (`Commit 465.90 MiB`) / `1.45%` (`gui 214.15 MiB`) |
| `probe[gui]` Go 堆与缓存 | `heap_alloc_max 62.25 MiB`, `entries=12, details=12, image_owners=12, origs=1` | `heap_alloc_max 94.08 MiB`, `entries=12, details=12, image_owners=12, origs=1` |
| 时延 (`ipc_to_visible_ms` / `original_ready`) | `508.1 ms` / `37.0–40.0 ms`（`interactive_op P50=439 ms`） | `678.8 ms` / `81.5–93.0 ms`（`interactive_op P50=402 ms`） |

### 2.4 关闭退出回收与隐藏/最小化留存（`fg-close-reclaim`，Track A `standard`）

| 阶段 / 衍生指标 | macOS (`phys_footprint`) | Windows (`PrivateWS` / `PrivateCommit`) |
|---|---|---|
| `pre-open-bg`（收敛后 15s） | `76.69 MiB`（`unicd 29.30 MiB`） | `31.51 MiB` / `175.39 MiB`（`unicd 11.88 / 58.49 MiB`） |
| 5 轮打开中位 → 关闭后中位 | `118.11 MiB` → `74.83 MiB`（`unicd 26.83 MiB`） | `162.03 MiB` → `61.25 MiB` / `203.33 MiB`（`unicd 39.99 / 86.34 MiB`） |
| `reclaimed_bytes` / `reclaim_ratio_pct` | `43.29 MiB` (`45,393,168 B`) / **`100.00%`**（`residual -1.87 MiB`） | `100.80 MiB` (`105,693,184 B`) / **`77.23%`**（`residual +29.73 MiB`） |
| 5 轮关闭后斜率（`LeakCheck` 阈值 `1 MiB/round`） | `+0.56 MiB/round` (`PASS`) | `+0.32 MiB/round` (`PASS`) |
| `pre-hide-visible` → `gui-hidden-retained` | `120.43 MiB` → `118.90 MiB`（留存变化 `-1.53 MiB`） | `162.19 MiB` → `162.37 MiB`（留存变化 `+0.17 MiB`） |

### 2.5 快捷面板生命周期（`qp-lifecycle`，`standard`）

| 子项 / 指标 | macOS (Track A / Track B) | Windows 11 (Track A / Track B) |
|---|---|---|
| 冷启 `qp-cold-open` → `standby` | A: `86.32` → `84.51 MiB`<br>B: `132.67` → `126.81 MiB` | A: `94.96` (`Commit 189.86`) → `90.18 MiB`<br>B: `121.25` (`Commit 216.51`) → `116.76 MiB` |
| 热复用 `qp-warm-open` → `qp-warm-closed` | A: `157.91` → `135.43 MiB`<br>B: `221.97` → `201.14 MiB` | A: `148.51` → `138.28 MiB` (`Commit 280.05`)<br>B: `174.27` → `165.94 MiB` (`Commit 306.89`) |
| 热开关第 1 → 10 轮闭后差 & 斜率 | A: `+16.69 MiB`, **`-1.18 MiB/round`** (`PASS`)<br>B: `-6.03 MiB`, **`-0.77 MiB/round`** (`PASS`) | A: `+6.28 MiB`, **`+1.72 MiB/round`** (`LeakCheck.Passed=false`)<br>B: `+19.70 MiB`, **`+1.71 MiB/round`** (`LeakCheck.Passed=false`) |
| Handle / FD / GDI / USER 漂移 | FD delta: `-1` (A), `-3` (B) | Handles delta: `-18` (A), `-1` (B); GDI 稳定 `53`, USER 稳定 `21` |
| `cold_ipc_to_visible_ms` / `cold_search` | A: `274.8 ms` / -；B: `330.0 ms` / `336.1 ms` | A: `390.0 ms` / -；B: `581.3 ms` / `607.6 ms` |
| 10 轮热开 `ipc_to_visible_ms` P50 / P95 | A: `117.8` / `136.3 ms`；B: `126.7` / `143.1 ms` | A: `290.1` / `483.2 ms`；B: `359.4` / `502.4 ms` |
| Track B `ipc_to_search_ready_ms` P50 / P95 | `132.1` / `149.1 ms` (count=10) | `362.2` / `507.1 ms` (count=10) |
| Track B `ipc_to_original_ready_ms` (count=31) | P50 `47.0` / P95 `572.0` / Max `626.0 ms` | P50 `55.0` / P95 `157.5` / Max `542.0 ms` |

---

## 3. 客观瓶颈与归因洞察（仅作观测，不改生产代码）

1. **Windows `qp-lifecycle` 10 轮热开关呈 `+1.71–1.72 MiB/round` 正斜率**：Handle（`-18 / -1`）、GDI（`53`）、USER（`21`）均未泄漏；增量全部体现在 `quick-panel.exe` 与 `unicd.exe` 的 `PrivateWorkingSet` / `PrivateCommit`（Track B probe 记录 `quick-panel` `heap_alloc_max 75.82 MiB`、`heap_inuse_last 70.87 MiB`、`details=20, image_owners=12`）。该信号清晰触发了 `LeakCheck`（> `1 MiB/round`），建议在后续独立优化专项中排查 Windows 端 Quick Panel 缩略图/D3D11 纹理释放时机与 Go GC/scavenger 周期。
2. **Windows `fg-close-reclaim` 收回率 `77.23%` vs macOS `100.00%`**：`gui.exe` 关闭后已释放约 `~100 MiB`，`+29.73 MiB` 残留几乎全部来自 `unicd.exe` 从纯后台初始 `11.88 MiB` 升至历史查询/读图后的 `39.99 MiB`（`Commit 86.34 MiB`）且在 15s 静置期内维持稳态，多轮斜率仅 `0.32 MiB/round` 未持续增长。
3. **跨平台最小化 / 隐藏语义差异一致表现为"全额留存"**：`gui-hidden-retained` 相对 `pre-hide-visible` 在 macOS 为 `-1.53 MiB`、在 Windows 为 `+0.17 MiB`，关闭窗口（退出 `gui` 进程）是释放显存/GPU 与 Go 堆的主要边界。
4. **`standard`（12 张 2K PNG）对静态待机前台的影响**：macOS 下 `fg-visible-idle` `standard` 与 `empty` 稳态近乎一致（`115.80` vs `114.90 MiB`）；Windows 下则从 `69.76 MiB` 升至 `161.90 MiB`（`+92.14 MiB`，其中 `gui` `97.18 MiB`、`unicd` `41.07 MiB`）。

---

## 4. 诚实未测边界（`NOT_RUN`）

- **物理 x64 (`windows/amd64`) 主机及 `windows/amd64` 仿真对比**：本次实测为 Parallels 虚拟机上的原生 `windows/arm64`（`emulated=false`）。
- **物理 Intel (`darwin/amd64`) macOS 主机**：本次实测为物理 Apple Silicon (`darwin/arm64` M4)。
- **物理全局热键与显示器首帧光子延迟**：所有时延均为程序内 IPC/Automation 派发至角色就绪/窗口可见/输入就绪的 `ipc_to_*_ms`，不含物理键盘驱动与显示器合成器扫描延迟。
- **24 小时睡眠/唤醒与长周期网络同步压测**：本次覆盖标准周期（`default` 3 轮 + 10 轮热开关），所有场景禁用远端同步（`sync.enabled=false`）。
