# 经验收件箱

本文件只收集 `UniClipboard/UniClipboard` 中有证据支持的临时经验，不是新的权威规则集。下面的模板不是历史事故记录；已核对的临时条目及证据边界见文末。

## 使用与归属

- 开工前按任务范围和触发条件阅读相关条目，并核对对应的权威文档与当前代码。未验证的推断不能当作既定事实。
- 回归、审查反馈或项目特有问题解决后，提出新增或更新建议；记录实际证据、适用边界和验证结果，不为凑数量编造事故或通过记录。
- 本文件不覆盖 [AGENTS.md](AGENTS.md)、[VISION.md](VISION.md) 或所属领域的规则。发现冲突时记录双方链接、当前行为与待决问题，按 [项目记忆来源](docs/agent/project-memory.md) 和 [文档职责](docs/AGENTS.md) 核实，不静默改写权威来源。
- 协议、持久化、迁移和引擎内部修复仍归 `UniClipboard/Engine`，见 [工作区归属](crates/AGENTS.md)。这里只记录证据与交接建议；不得在桌面仓库复制引擎规则或实现补丁。本流程不授权修改 Engine 或 UniClip。
- 严重安全或数据丢失风险首次发现就立即向用户报告并提出处置与交接建议；通过已获授权的渠道通知负责维护者，不等待重复事故、下次巡检或规则晋升。证据须脱敏，不写入密钥、令牌或用户内容。

## 条目模板

复制以下模板到文末，每条只表达一个可执行规则。日期使用实际的 `YYYY-MM-DD`；未执行的验证明确写“未运行”及原因。

```markdown
### LESSON-YYYYMMDD-NN — 简短标题

- 状态：provisional | promotion-proposed | promoted | retired
- 范围 / 触发条件：仓库内路径、操作或失败条件；不适用范围
- 具体规则：遇到什么条件时，必须做什么或避免什么
- 原因：已确认的原因；未确认的推断与置信度分别说明
- 证据：PR / 审查评论 / issue / 固定提交链接及相关路径；脱敏复现记录
- 验证：命令或复现步骤、提交 SHA、环境、实际结果；未运行项及原因
- 日期：首次记录 YYYY-MM-DD；最近证据 YYYY-MM-DD；最近复核 YYYY-MM-DD
- 去重 / 冲突：相关经验 ID 或权威规则链接；差异和待决问题（无则写无）
- 晋升目标：所属 AGENTS.md / 专题文档 / 自动检查的具体路径与章节；负责领域
- 下一步：需要补充的证据、验证或审查
- 关闭记录：晋升或退役日期、原因、目标规则 / 检查 / 替代条目及接受该变更的 PR 链接
```

## 去重、晋升与退役

1. 新增前搜索本文件、目标领域的 `AGENTS.md` 和关联文档。相同根因、触发条件和规则复用同一 ID，追加新证据与最近日期；关联 PR 或重复评论不算独立事故。不同适用边界需明确区分。
2. 证据不足时保持 `provisional` 并写明待验证项。已有规则足以覆盖的，只引用该规则并提出补充检查或改善入口的建议，不创建平行规则。
3. 当证据支持稳定、项目特有且可执行的规则时，设为 `promotion-proposed`，提出具体目标和修改内容。重复发生能增强证据，但没有固定次数门槛；高风险发现立即处理。
4. Rust 工作区的约定与反模式沿用 [learn 技能](.agents/skills/learn/SKILL.md)，包括资格判断、去重、章节容量与不自动提交的规则。其他领域放入所属 `AGENTS.md` 或专题文档；产品和架构决策仍由 `VISION.md` / ADR 承载，不把收件箱变成决策来源。
5. 能可靠自动检测的错误，优先提出回归测试、lint 或 CI 检查，并验证检查能捕获失败且接受正确行为；不要重复写成 learn 规则。检查尚未落地时保留待办，不声称已受保护。
6. 目标规则或检查经审查接受后，标记 `promoted`，记录日期、接受变更的 PR 和准确目标；以后以目标为准。被证伪、代码已移除、被替代或已重复的条目标记 `retired` 并写明原因与替代链接。保留 ID 和简短证据链，不删除历史来掩盖冲突；必要时将长篇理由移到所属文档并链接。

## 每日巡检程序（手动调用）

这是一份可复用的操作程序，不安装或启用调度器。调用时指定仓库、基准提交或上次巡检时间、截止提交与 UTC 时间；首次默认查看最近 24 小时，并明确覆盖范围。本程序只产出报告和修改建议，不自动提交、发布、合并或修改安全设置。

1. 确认当前仓库为 `UniClipboard/UniClipboard`，记录 HEAD、分支和工作区状态；保留无关修改。读取根入口、相关经验、目标领域规则和 learn 技能。
2. 查看指定范围内的提交和差异，以及可访问的 PR 审查、回归 issue、测试与 CI 结果。可用 `git log --since='24 hours ago' --oneline` 初筛，再按 SHA 阅读差异；已有基准时用 `git log BASE..HEAD`。时间筛选不能证明覆盖所有活动，记录缺失的访问权限、未覆盖分支和不可用证据。
3. 将每个候选结论追溯到实际证据，区分新经验、同一 ID 的复发、既有规则未执行和文档冲突。检查代码是否已修复、规则是否已被测试覆盖；不要把评论本身当作已验证根因，也不要重复统计同一事故。
4. 用模板提出最小条目变更；对晋升给出目标路径、建议规则或检查、理由和验证计划。跨仓库问题只提出交接；严重安全或数据丢失风险立即报告，不等巡检结束。
5. 输出以下报告；没有新发现也明确写“无”，不制造经验。需要实际修改时遵循本次任务授权，提交和发布仍是独立授权动作。

```markdown
### 经验巡检 — YYYY-MM-DD

- 覆盖：仓库、分支、基准 / 截止 SHA、UTC 时间范围、工作区状态
- 已查证据：提交 / PR / 审查 / issue / CI 链接（CI 对应的 SHA 与结果）
- 新经验：候选 ID、规则、证据与验证状态；无则写无
- 复发：已有 ID、新证据、独立事故或同一事故的后续
- 冲突 / 退役：双方来源、适用边界、处理建议
- 晋升建议：ID、具体目标、建议修改或自动检查、理由与待运行验证
- 紧急发现：严重程度、证据、已报告对象或尚待授权的交接
- 限制与下一步：未运行检查、无法访问的证据、待审查事项
```

## 经验条目

### LESSON-20261012-01 — History 验收必须跨过搜索接口的单次上限

- 状态：provisional
- 范围 / 触发条件：修改 `apps/gui-go/frontend/src/hooks/useLiveSearch.ts` 的加载窗口或 History 列表验收；仅限桌面搜索客户端，不改变 Engine 搜索协议。
- 具体规则：加载超过接口单次上限的窗口时，使用有上限的请求和递增 offset；验收数据必须超过 200 条，并断言能到达最旧条目。仅验证窗口 limit 增长或首屏不足以证明分页正确；接口上限变化时同步调整边界夹具。
- 原因：静态核对确认，修复前 hook 只增长 limit，不传 offset；接口在 `crates/uc-webserver/src/api/search.rs` 声明单次最多 200 条。修复后的 `querySearchWindow` 分页请求，按原始返回条数推进 offset，再按 entryId 去重。客户端与接口合同不一致有代码证据；本次未独立复现历史运行故障。
- 证据：[修复及 230 条数据的原生 E2E 断言](https://github.com/UniClipboard/UniClipboard/commit/c4e21482cbfe775f339e99a335e7e7a79b849461)；[此前的审查发现](https://github.com/UniClipboard/UniClipboard/pull/1963#pullrequestreview-5485938888)；[PR #1964 的验证说明](https://github.com/UniClipboard/UniClipboard/pull/1964)。三者属于同一事故，不计为三次复发。
- 验证：本次对固定 main `386c8226d533cab39542422e869aa086ca724ebf` 静态读取前后差异、API 上限、`useLiveSearch.window.test.ts` 和 `kit-paging-past-200`；窗口单测只验证 limit 状态，没有调用真实 daemon。历史 [CI 38195640572](https://github.com/UniClipboard/UniClipboard/actions/runs/38195640572) 在 PR head `41a6f427dc1aaefaeb9a712eb6135681be83cdfc` 的 frontend 检查通过，CLI E2E 被跳过，不能替代原生分页验收。PR 描述报告修复前卡在第 200 条、修复后 48/48；原始运行工件本次未取得，不把该报告写成本次复跑结果。
- 日期：首次记录 2026-10-12；最近证据 2026-10-12；最近复核 2026-10-12。
- 去重 / 冲突：接口已有上限说明；[前端规则](docs/agent/frontend-ui-rules.md) 未覆盖跨上限验收。保留现有 `kit-paging-past-200` 检查，不另建平行实现；无权威规则冲突。
- 晋升目标：优先明确 `apps/gui-go/e2e/history_kit_ui_run.py` 现有原生检查的执行与工件保留入口；确需文字说明时放入 `apps/gui-go/README.md` 的 History 验收说明，不写入 `crates/AGENTS.md`。
- 下一步：在专用 macOS 主机对固定提交运行 `apps/gui-go/build.sh e2e` 与 `python3 apps/gui-go/e2e/history_kit_ui_run.py --out <隔离工件目录>`，保留越过第 200 条的断言、源码 SHA 与原始结果；同时核实 Windows 对应边界。此环境缺少原生宿主与构建依赖，未运行。
- 关闭记录：未晋升、未退役；等待审查与原生证据。

### LESSON-20261012-02 — Windows profile 隔离要验证 WebView 自身存储

- 状态：provisional
- 范围 / 触发条件：Windows Wails 宿主的命名 profile、便携模式或原生 History E2E；不外推到 Linux WebKitGTK，也不改默认安装 profile 的存储迁移策略。
- 具体规则：确认命名 / 便携 profile 将 `WindowsOptions.WebviewUserDataPath` 设为其数据根下的 `webview2`，并在新原生 profile 首次打开时断言 History 布局键为空。仅设置 `UC_PROFILE` 或测试脚本中的 `WEBVIEW2_USER_DATA_FOLDER` 不能作为 WebView 已隔离的证据；默认安装 profile 保留既有路径，避免丢失偏好。
- 原因：静态差异确认旧 `main.go` 未设置宿主的 WebView 数据路径；修复加入 `webviewDataPath()` 并删除测试环境变量，新增 `assertNoStoredLayout()`。PR 对 Wails 显式路径覆盖环境变量、旧运行共享 localStorage 的解释仍是历史报告；本次没有原生复跑，不能宣称所有 profile 或平台已验证隔离。
- 证据：[宿主路径、测试驱动与空布局断言的修复](https://github.com/UniClipboard/UniClipboard/commit/01024320bd22302024880005bbe5229c5ba71f86)；[PR #1965 的原生回归说明与未运行项](https://github.com/UniClipboard/UniClipboard/pull/1965)。同一 PR 中的 tooltip 修复是另一个问题，不作为存储隔离的复发证据。
- 验证：本次在固定 main `386c8226d533cab39542422e869aa086ca724ebf` 核对 `main.go`、`webview_data.go`、`windows_history_kit_remote.py` 与 `history-kit-browser.mjs`。历史 [CI 38202705710](https://github.com/UniClipboard/UniClipboard/actions/runs/38202705710) 在 PR head `4d735f6b67938fec582b5398565d0a8501589ab2` 的 frontend 和 Go host compile 检查通过；没有原生 Windows 隔离检查。PR 报告 ARM64 原生 28/28、首启 2/2，原始工件本次未取得；x64、默认安装版升级及 Linux 存储隔离仍未据此验证。
- 日期：首次记录 2026-10-12；最近证据 2026-10-12；最近复核 2026-10-12。
- 去重 / 冲突：[宿主规则](apps/gui-go/AGENTS.md) 已要求唯一 profile 与便携沙箱，但未要求验证 WebView 布局键。现有断言已覆盖新 profile 空布局，优先复用；[Linux 升级存储说明](docs/architecture/gui-go-tauri-upgrade-continuity.md) 的平台范围不同，无需覆盖或改写。
- 晋升目标：`apps/gui-go/README.md` 的 Windows 原生验收说明及现有 `apps/gui-go/e2e/windows_history_kit_remote.py` 验收，不写入 Rust 工作区约定。
- 下一步：在专用 Windows 主机保留两个独立 profile 的实际目录、空布局断言和清理记录；另测默认安装 profile 升级后偏好保留。已有测试是证据入口，不等于所有场景已覆盖；本环境无 Windows/WebView2，未运行。
- 关闭记录：未晋升、未退役；等待审查与原生证据。

## 首次七日巡检记录 — 2026-10-12

- 覆盖：UTC `2026-10-05 07:20:18` 至 `2026-10-12 07:20:18`，固定 main 为 `386c8226d533cab39542422e869aa086ca724ebf`；窗口前的第一父提交为 `3a781525d34f6f426cb54f994023cf3f2f48522b`。枚举两者之间 118 条第一父提交，按提交时间筛选，重点阅读下表候选的相关差异、当前规则及可访问的讨论 / CI。巡检开始时 `docs-lessons-workflow` 工作区干净，HEAD 为 `58ead3de41c593e50eddde2705279b1bb1fe1808`。
- 新经验：上面两条；无已确认的独立复发、晋升或退役。没有在本次扫描中确认新的严重安全或数据丢失问题；这不等于完成安全审计。

| 候选 | 处理与证据边界 |
| --- | --- |
| History 超过 200 条、Windows WebView profile | 分别记为 01 / 02；依据实际代码差异和断言，原生运行报告与本次核验分开记录。 |
| [#1956 启动诊断](https://github.com/UniClipboard/UniClipboard/commit/5bc6dc8934dce3c3f0b257fb45d55ef1a98afe2b) | 不新增：独立启动服务读取已由 [启动进度文档](docs/architecture/startup-upgrade-progress.md) 规定；本次核对 Go client / probe 的相关修复，不审计该 PR 全部功能。 |
| [#1947 发布与运行验收](https://github.com/UniClipboard/UniClipboard/commit/aadc5c79af9dd0281b8bdf59e71736c34e51f934) | 不新增：已在 [Linux 打包文档](docs/architecture/gui-go-linux-ci-packaging.md) 明确打包成功不代表运行验收成功；反射返回值修复是通用 Go 知识。 |
| [#1955 CI 路径路由](https://github.com/UniClipboard/UniClipboard/commit/95d0d8f1c85a20dd4adb0db30dc0835b7ca4556b) | 不新增：已有 `.github/ci-paths.yml` 与 `scripts/__tests__/pr-check-routing.test.ts`；优先维护自动检查，不抄成经验规则。 |
| [#1888 安全诊断与进程验收](https://github.com/UniClipboard/UniClipboard/commit/9c76e0f4796aedb644bc8ed78a839d43b5032b2b) | 不新增：日志脱敏已属 `VISION.md` 底线，来源与运行证据边界已在 [验收说明](scripts/e2e/clipboard-startup/README.md)；不把已修复的日志问题宣称为新发现。 |
| [#1961 LAN 通知](https://github.com/UniClipboard/UniClipboard/pull/1961) | 交接待核验，不建立桌面协议规则：新增测试的断言已读；PR 明确报告旧 Engine pin 失败、临时 override 通过。固定 main 的 `Cargo.toml` 仍为该旧 pin `416fcca93ea6658b4e5918bafd464c6f850a17bc`；本次未运行，不能宣称正式依赖已修复。需在另行授权的 Engine / 依赖更新工作中核验。 |

- 限制：这是按风险选取候选的扫描，不是 118 个提交的逐行完整审计；未遍历所有未合并分支、issue 或私有事故记录，未取得 PR 报告引用的本机原始工件。没有运行 macOS / Windows 原生验收、Rust 或前端应用测试；此环境无相应宿主、Cargo、Bun 和项目依赖。CI 历史只核对上述精确 PR head 的作业结果，不把绿色摘要当成原生端到端证明。
- 下一步：审查两条临时经验及其验证计划；复用现有检查补齐固定提交的原生证据后再决定是否晋升。保留所有权边界，不在本 PR 修改产品、Engine pin 或自动化调度。
