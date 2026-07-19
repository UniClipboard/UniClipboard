# Plan 001：固化四平台完整 P2P 产品与架构决策

> **执行要求**：逐步执行并运行每个验证命令。出现停止条件时停止，不得自行缩减成“移动端继续使用 LAN HTTP”。完成后更新 `plans/README.md` 中本计划状态。
>
> **漂移检查**：`git diff --stat 1c229e9e1 -- README.md README_ZH.md VISION.md docs/architecture/adr-005-uc-engine-extraction.md docs/architecture/mobile-sync-connect-uri.md docs/packaging/mobile-core-build-release.md .planning/research/uc-mobile-spike-plan.md .planning/research/uc-mobile-goal-b-migration-plan.md .planning/phases/100-goal-b-mobile-sync-core-migration .planning/2026-06-25-dual-channel-file-sync-dedup-design.md`

## 状态

- **优先级**：P0
- **工作量**：S
- **风险**：LOW
- **依赖**：无
- **类别**：direction
- **计划基线**：`1c229e9e1`，2026-07-19

## 为什么必须先做

执行本计划前，项目文档同时保留两套互相冲突的决定。`docs/architecture/adr-005-uc-engine-extraction.md` 主张移动端前台运行完整节点；`VISION.md` 和 `.planning/research/uc-mobile-spike-plan.md` 后来把移动端锁定为 LAN HTTP 客户端。新的明确决定是：桌面、HarmonyOS、Android、iOS 都运行同一完整 P2P 核心；移动产品可以额外提供用户显式选择的独立 LAN HTTP 兼容通道。

## 基线事实

- `VISION.md:64` 把“移动端无法运行 iroh full node”列为锁定决定。
- `docs/architecture/adr-005-uc-engine-extraction.md:33-39` 已描述“移动端前台是完整节点”，但同文后部仍保留不支持移动端互联和失败回退到 LAN 的旧范围。
- `.planning/research/uc-mobile-spike-plan.md:139` 明确记录“移动端只做 mobile-sync，不做真正 P2P”。
- 项目现有离线语义是失败即报告、用户主动重发，不允许自动补投；更新文档时必须保持该规则。

## 范围

**允许修改**：

- `VISION.md`
- `README.md` 与 `README_ZH.md` 中的当前版本/目标架构说明
- `docs/architecture/adr-005-uc-engine-extraction.md`
- `docs/packaging/mobile-core-build-release.md`
- `.planning/research/uc-mobile-spike-plan.md`
- `.planning/research/uc-mobile-goal-b-migration-plan.md`
- `.planning/phases/100-goal-b-mobile-sync-core-migration/` 中的决策记录
- `.planning/2026-06-25-dual-channel-file-sync-dedup-design.md` 中引用旧方向的范围说明
- `docs/architecture/mobile-sync-connect-uri.md`
- 必要时 `docs/README.md` 中对应索引
- `plans/README.md` 中本计划状态

**禁止修改**：

- 任何 Rust、Swift、Kotlin、ArkTS、TypeScript 源码
- 当前 LAN HTTP 行为或发布流程
- 离线自动重发语义

## 步骤

### 1. 更新产品锁定决定

把 `VISION.md` 的移动端决定改为：四个平台共享完整 P2P 节点能力；桌面可常驻，移动节点按系统给予的运行窗口上线，暂停时正常离线，恢复后使用原身份重连。明确“不承诺永久后台在线”不影响其对等节点身份。

**验证**：`rg -n "移动端无法运行|Mobile 走独立 LAN|完整 P2P|对等节点" VISION.md`，预期旧断言为零匹配，新决定至少一处匹配。

### 2. 让 ADR-005 成为当前决定

将 ADR 状态改为接受，并修订以下旧范围：

- 移除“移动端之间不互通”的限制。
- 移除移动可行性失败后退回 LAN HTTP 的方案。
- 把生命周期改为 `start -> quiesce(deadline) -> suspend -> resume -> shutdown`。
- 明确同一时刻只允许一个节点，但同一进程可反复启动和停止。
- 明确平台差异只限系统接入和在线时长，不允许协议、加密、内容能力分叉。
- 保持离线不自动补投、用户主动重发的现有产品语义。

**验证**：`rg -n "退回 LAN|不承诺 mobile|mobile ⇄ mobile|start.*quiesce.*suspend.*resume" docs/architecture/adr-005-uc-engine-extraction.md`，预期只保留带历史说明或明确否决含义的 LAN 文本，生命周期新定义存在。

### 3. 明确旧移动方案的独立兼容身份

在移动 spike 与连接 URI 文档开头标明其是用户显式选择的独立 LAN HTTP 兼容通道，写明它与完整 P2P 核心分别演进、分别发布，不得自动回退或替代完整 P2P 验收。不得删除现有协议说明，因为已发布客户端仍需要它。

**验证**：`rg -n "兼容|显式选择|独立|自动回退" .planning/research/uc-mobile-spike-plan.md docs/architecture/mobile-sync-connect-uri.md`，预期两份文档都有明确标记。

## 测试计划

- 运行 Markdown 链接和格式检查（若仓库无专用命令，至少运行下方检查）。
- 检查所有新增代码块都有语言标识。
- 检查文档未引入机器绝对路径。

**验证**：

````bash
git diff HEAD --check
if git diff HEAD -U0 -- README.md README_ZH.md VISION.md docs/architecture/adr-005-uc-engine-extraction.md docs/architecture/mobile-sync-connect-uri.md docs/packaging/mobile-core-build-release.md .planning/research/uc-mobile-spike-plan.md .planning/research/uc-mobile-goal-b-migration-plan.md .planning/phases/100-goal-b-mobile-sync-core-migration .planning/2026-06-25-dual-channel-file-sync-dedup-design.md | rg '^\+.*(/Users/|/Volumes/|/private/var/|/tmp/|[A-Za-z]:\\)'; then exit 1; fi
for file in plans/*.md README.md README_ZH.md VISION.md docs/architecture/adr-005-uc-engine-extraction.md docs/architecture/mobile-sync-connect-uri.md docs/packaging/mobile-core-build-release.md; do
  awk 'BEGIN { open = 0; bad = 0 } /^```/ { if (!open) { if ($0 == "```") bad = 1; open = 1 } else { open = 0 } } END { if (open) bad = 1; exit bad }' "$file" || exit 1
done
````

预期所有命令均退出 0，且路径检查没有输出。

## 完成标准

- [x] 产品方向只剩一个当前决定：四平台完整 P2P。
- [x] 移动暂停等于节点离线，不被写成降级 LAN 客户端。
- [x] LAN HTTP 是用户显式选择的独立兼容通道，不自动回退且不替代 P2P 验收。
- [x] 离线不自动补投规则未改变。
- [x] `git diff --check` 通过。

## 停止条件

- 产品要求 iOS 在普通 App Store 权限下永久后台在线。
- 新决定要求中心服务器保存剪贴板或文件内容。
- 文档修改需要同时改变当前运行行为才能保持自洽。
