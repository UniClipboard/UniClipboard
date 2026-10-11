# PR 检查诊断与问题分类（2026-10-11）

## 范围与口径

- 诊断分支：`diagnose/pr-check-audit-2026-10-11`。
- 配置基线：`137af675d`；样本为最近 20 个已合并 PR，合并时间覆盖 2026-10-09 至 2026-10-11（UTC）。
- 通过 GitHub CLI 读取 PR 文件列表、最终提交的检查汇总、运行日志、主分支规则集；结合当前工作流配置核对。
- 检查汇总包含同一提交因 PR 描述编辑、关闭事件产生的重复任务，也包含合并后的任务及外部状态；不能等同于合并前执行的构建数量。
- 快照共 716 条检查记录，单 PR 24–52 条，中位数 37；其中 171 条为跳过状态。跳过项主要是界面噪声，不能按完整构建计算成本。
- 11/20 个 PR 的查询快照含失败检查，不代表这些失败全部发生在合并之前。最新 #1954 在合并后查询时，Rust、Windows Quick Panel、CLI E2E、Membership E2E 仍在运行。
- PR 文件列表对 #1940、#1936 返回 100 项，不能把它当作完整变更文件总数。运行时间是任务墙钟时间，不是计费金额。

## 结论

检查确实存在浪费，但首要问题是职责和门禁没有收敛：触发范围过宽、重复执行前端测试、后台自动化混入检查列表，而主分支没有必需状态检查。安全审计和状态收敛 E2E 的失败有具体依据，应处理失败原因，不能用删检查使其消失。

## 问题分类

| 编号 | 优先级 | 类别 | 证据与根因 | 建议 |
| --- | --- | --- | --- | --- |
| CI-01 | 高 | 合并门禁缺失 | 活跃规则集 `13587567` 仅要求 PR、线性历史、禁止删除/强推，没有 `required_status_checks`；传统保护接口返回未保护。#1954 的描述校验在合并前及关闭后均失败，重型检查合并后仍在跑 | 先定义明确的质量门禁，再设置始终出现的汇总检查；汇总必须验证相关任务确实成功，不得让失败、取消或意外跳过变成成功 |
| CI-02 | 高 | 路径触发范围过宽 | `pr-check.yml` 的 `code`、`rust` 都覆盖 `apps/**`；`coverage.yml`、`membership-e2e.yml` 同样覆盖整个 apps。#1954 只改 Go CLI、Python 验收和站点文档，却启动前端测试、Rust 覆盖率、Rust 编译、两架构 Windows 面板编译和 Membership E2E | 建立前端、Go CLI、Go 宿主、Rust 宿主、Quick Panel、协议/集成测试独立变更集合；保留 Go CLI 所需集成测试，取消无关前端与面板任务；依赖锁和 Engine 固定提交变更扩展到受影响消费者 |
| CI-03 | 中 | 重复测试与构建 | `Frontend Tests` 执行 `vitest run`，`Frontend Coverage` 执行同一配置的 `vitest run --coverage`；`Frontend Type Check` 和 `Go Host Compile Check` 又分别构建同一 Wails 前端，后者的 `check:host-boundary` 再运行 tsc | 前端测试与覆盖率合并为一个任务，保留 main 的覆盖率基线；评估复用同一提交的前端构建产物，保留必要的宿主契约验证 |
| CI-04 | 中 | 无关诊断扫描 | `react-doctor.yml` 对所有 main PR 运行，没有路径筛选。#1938 只改发布镜像工作流也运行 React Doctor。5 个 `Analyze` 来自 GitHub 动态 `Code Quality`，并非仓库 YAML；包括 Go、Java/Kotlin、JS/TS、Python、Ruby | React Doctor 限前端变更；在 GitHub Code Quality 设置中核对实际语言及扫描范围，避免无关语言逐 PR 扫描；不能仅删本地 YAML 解决动态扫描 |
| CI-05 | 中 | 生命周期任务污染检查列表 | `issue-lifecycle.yml` 的 4 类任务共享事件工作流；opened/edited/closed 都执行描述校验，其他任务多数跳过。#1947 因多次事件有 4 条同名描述校验及大量生命周期跳过项 | 分离 PR 描述校验和合并后/发布后的自动化；关闭事件只执行生命周期更新，描述编辑仅触发校验 |
| CI-06 | 中 | 过期任务不取消 | PR Check、Coverage、Frontend Coverage、Membership E2E、React Doctor 等未配置并发取消；Package Linux GUI 已有按 PR 取消机制 | 给可取消的只读 PR 验证按工作流与 PR 编号设置并发组，取消旧提交任务；发布和外部写入流程保留独立策略。样本尚未量化旧提交重叠成本 |
| CI-07 | 高 | 真实依赖审计失败及归属混乱 | 样本有 8 个 PR 的 cargo audit、9 个 PR 的 docs-site 审计失败；任一受监控依赖文件变化都会运行完整 Rust 和 3 个 Bun 项目审计矩阵 | 不删除审计。PR 按依赖项目运行，定时继续全仓审计；分别跟踪已有漏洞与本次引入漏洞，修复或按现有规则论证例外，不能扩大忽略列表掩盖问题 |
| CI-08 | 高 | E2E 重复失败 | #1939、#1945 的 CLI E2E 均约 38.6 分钟失败，都出现 `notified_host_converges_after_pending_group_update_is_delivered` 初始收敛超时；#1939 另有杀进程重启后发送得到 offline。样本还有 #1936 的 CLI E2E 失败，但未逐项分析其日志 | 单独诊断状态收敛及重启测试，定位产品实现、测试时序、环境问题；现有证据不足以直接归为偶发测试，也不足以指定修复位置。Engine 内部缺陷应在 Engine 仓修复 |
| CI-09 | 中 | 触发遗漏 | API 客户端漂移校验放在 `code` 下，但其过滤器没有 `schema/**`；仅改 schema 时该校验跳过。Go CLI 没有独立编译任务，主要借 Rust/E2E 的宽泛触发覆盖 | 在收窄范围之前补齐 schema、生成配置、E2E 脚本、Go 模块依赖等真正输入；给 Go CLI 明确的编译/静态检查和验收职责 |
| CI-10 | 中 | 格式检查改变被测提交 | `format.yml` 自动提交格式化修改；注释明确 GITHUB_TOKEN 推送不会自动触发工作流，同时原检查以失败结束 | 改为只读格式校验，修复由作者或本地工具提交；避免 PR 新提交没有对应的一组完整检查 |

## 失败日志的具体证据

- [#1951 docs-site 审计](https://github.com/UniClipboard/UniClipboard/actions/runs/38114703687/job/114397179518)：生产依赖审计报告 9 个漏洞（3 高、4 中、2 低），涉及 Next.js、KaTeX、sharp、source-map-js。此处陈述日志结果，未进一步判断各漏洞在部署方式中的可利用性。
- [#1950 Rust 审计](https://github.com/UniClipboard/UniClipboard/actions/runs/38065880078/job/114253307772)：`libcrux-kem 0.0.9` 被报告 `RUSTSEC-2026-0330`、`RUSTSEC-2026-0331`；日志列出的修复版本为 `>=0.0.10`。其他 PR 是否完全相同尚未逐个核对。
- [#1945 CLI E2E](https://github.com/UniClipboard/UniClipboard/actions/runs/38041232895/job/114181740537)：`space_device_update_convergence.rs:174` 初始收敛未完成；最后状态的设备组更新为 `needs_attention` / `device_security_update_rejected`。
- [#1939 CLI E2E](https://github.com/UniClipboard/UniClipboard/actions/runs/38030382303/job/114149893846)：同一收敛测试失败，另有 `space_creator_keeps_network_identity_after_killed_restart` 发送结果为 offline。
- [#1954 描述校验](https://github.com/UniClipboard/UniClipboard/actions/runs/38117659284/job/114405698890)：合并前因 `Closes #1952` 失败；关闭事件再次执行同一校验并失败。符合当前生命周期规则，不属于误报。
- #1940 两个 Linux 打包任务均失败；抽查 amd64 日志确认失败退出，但本次未完成其具体根因诊断，不能归为无用检查。

## 应保留的验证职责

- 前端类型、实际发布 bundle、单元测试与覆盖率；测试及报告可由同一执行产生。
- Go/Wails 宿主跨平台编译与宿主命令契约，Rust 工作区编译。
- `cargo check -p quick-panel` 的独立编译检查：工作区 feature unification 会掩盖独立包依赖问题，不能简单当作重复删掉。
- Windows x64/ARM64 原生面板编译：Linux 无法验证 win32 代码，保留但收窄触发。
- CLI 与设备成员关系 E2E：分别有用户接口与协议回归职责，需按场景比较才能合并，不能只因都启动 daemon 就认定重复。
- 更新器签名/清单 E2E、Linux 打包及运行验收、安全审计：有独立风险覆盖；#1947 已把发布打包和运行验收拆开，应继续沿用。
- Codecov 在 main 的基线报告不能只因 PR 已执行而移除。
- #1953 已调整 Vercel 忽略构建配置；后续 PR 仍出现 Vercel 状态并不证明实际完整构建发生，需另查部署日志。本次不计入已证实的构建浪费。

## 建议实施顺序

1. 修复并追踪真实审计/E2E 失败，定义必需验证职责及汇总门禁；主分支规则调整作为独立的仓库设置变更。
2. 补齐遗漏输入，再拆分变更集合；特别保留 Go CLI 对 daemon 集成测试的依赖。
3. 合并前端测试与覆盖率，收窄 React Doctor 和 Windows 面板触发；动态 Code Quality 设置单独处理。
4. 分离生命周期自动化，加入只读验证任务的并发取消，调整自动格式提交机制。
5. 用文档、前端、Go CLI、Go 宿主、Rust、Engine 更新、打包、更新器各类 PR 验证触发矩阵；比较相同窗口的实际执行时间和失败信号。

初次诊断只保存证据，未修改工作流、主分支规则或外部应用设置，也未创建 GitHub issue。后续实施情况见文末。

## 样本清单

| PR | 标题 | 快照检查记录数 | 失败任务（不含 pending） |
| --- | --- | --- | --- |
| [#1954](https://github.com/UniClipboard/UniClipboard/pull/1954) | feat(cli-go): improve interactive device-group choices | 32 | Validate issue references ×2 |
| [#1951](https://github.com/UniClipboard/UniClipboard/pull/1951) | feat(devices): show per-device group key update delivery state | 37 | bun audit (docs-site) |
| [#1953](https://github.com/UniClipboard/UniClipboard/pull/1953) | fix(docs-site): skip unchanged Vercel preview builds | 26 | 无 |
| [#1950](https://github.com/UniClipboard/UniClipboard/pull/1950) | feat(quick-panel): GPUI quick panel on Windows | 39 | cargo audit；bun audit (docs-site) |
| [#1949](https://github.com/UniClipboard/UniClipboard/pull/1949) | fix(docker): include shared CLI module and honor Go toolchain | 28 | 无 |
| [#1947](https://github.com/UniClipboard/UniClipboard/pull/1947) | fix(ci): separate release packaging from runtime acceptance | 51 | 无 |
| [#1946](https://github.com/UniClipboard/UniClipboard/pull/1946) | release: v1.2.0-alpha.2 | 39 | cargo audit；bun audit (docs-site) |
| [#1945](https://github.com/UniClipboard/UniClipboard/pull/1945) | fix(ci): grant release build nested artifact read permission | 35 | CLI E2E Tests |
| [#1944](https://github.com/UniClipboard/UniClipboard/pull/1944) | release: v1.2.0-alpha.1 | 39 | cargo audit；bun audit (docs-site)；Vercel |
| [#1943](https://github.com/UniClipboard/UniClipboard/pull/1943) | fix(retirement): remove host leftovers and document acceptance gaps | 40 | cargo audit；bun audit (docs-site) |
| [#1942](https://github.com/UniClipboard/UniClipboard/pull/1942) | refactor: remove the uc-desktop crate | 35 | cargo audit；bun audit (docs-site) |
| [#1940](https://github.com/UniClipboard/UniClipboard/pull/1940) | refactor(gui-go): move the React frontend into apps/gui-go/frontend | 52 | cargo audit；Package amd64；Package arm64；bun audit (docs-site) |
| [#1939](https://github.com/UniClipboard/UniClipboard/pull/1939) | docs(uc-dev-cli): clarify diagnostic ownership and daemon boundaries | 37 | cargo audit；bun audit (docs-site)；CLI E2E Tests |
| [#1938](https://github.com/UniClipboard/UniClipboard/pull/1938) | fix(release): read the Desktop mirror's release notes from the release tag | 24 | 无 |
| [#1937](https://github.com/UniClipboard/UniClipboard/pull/1937) | ci(release): mirror Desktop installers with the GitCode release mirror Action | 24 | 无 |
| [#1936](https://github.com/UniClipboard/UniClipboard/pull/1936) | refactor(gui-go): generate the host command contract from the typed HostService (#1908) | 34 | cargo audit；bun audit (docs-site)；CLI E2E Tests |
| [#1934](https://github.com/UniClipboard/UniClipboard/pull/1934) | ci(release): gate the tag-triggered Go host release before any external write | 41 | 无 |
| [#1935](https://github.com/UniClipboard/UniClipboard/pull/1935) | ci: add a dispatch-only smoke test for the GitCode mirror Action | 24 | 无 |
| [#1930](https://github.com/UniClipboard/UniClipboard/pull/1930) | test(gui-go): validate Linux runtime dependencies and package formats beyond Ubuntu and Fedora | 39 | 无 |
| [#1929](https://github.com/UniClipboard/UniClipboard/pull/1929) | fix(gui): one Library layout, window frame and shortcut portal fixes from Linux acceptance (#1902) | 40 | 无 |

## 后续实施：前三类问题

- 触发范围：用 `.github/ci-paths.yml` 统一维护组件输入；Go CLI 保留 CLI/Membership E2E，前端和 Go 宿主不再误触发 Rust/Windows 面板任务；共享 Rust、Go 依赖及 schema 校验保留。
- 重复执行：前端测试与覆盖率统一执行；Go 宿主复用前端 bundle 和类型检查；缓存策略测试归入统一仓库工具测试，移除独立工作流。
- 检查噪声：PR 描述校验独立运行，合并后只更新关联问题状态；发布验证与 `/verify` 留在非 PR 生命周期工作流；React Doctor 限前端相关输入。
- GitHub 动态 Code Quality 扫描、Vercel 和 CodeRabbit 属于外部应用或仓库设置，本轮仓库工作流改动未调整其配置。
- 主分支必需状态检查、安全漏洞修复及 E2E 状态收敛故障属于其他分类，本轮未修改。

### 本地验证

- 仓库工具测试：22 个文件、219 个测试通过，其中路径与依赖回归测试 27 项。
- 前端测试及覆盖率：229 个文件、1466 个测试通过。
- 前端类型检查、实际发布 bundle 构建及 macOS 12.5 兼容性检查通过。
- 全部工作流通过 actionlint 校验（未启用外部 shellcheck/pyflakes）；修改过的测试通过 oxlint；依赖冻结安装及 `git diff --check` 通过。
- 尚未在 GitHub 运行新工作流；Go 跨平台编译及 artifact 上传/下载链路仍需远端运行验收。
