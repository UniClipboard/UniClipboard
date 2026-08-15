# Brief：CLI 设备信任核对适配

**日期：** 2026-08-14
**状态：** 已完成调研，待产品评审
**研究问题：** CLI 如何在没有 GUI 的环境中完整承接设备信任核对，同时保持交互安全、脚本稳定和产品语义一致？

## 建议

将 `uniclip member` 从旧的“成员移除进度”入口升级为完整的设备信任入口。提供独立的只读查询和明确决定命令：用户先读取后台给出的完整当前结果，再携带当前变化编号提交选择。交互式终端可以引导确认；JSON 模式和非交互环境不得弹出提示，也不得猜测用户意图。

## 关键发现

1. GUI 与 CLI 面向的是同一项安全决定，设备关系、允许选择和操作结果必须来自同一份后台事实，CLI 不能重新计算。
2. 当前 CLI 只有主动移除和旧收敛摘要。它能提示存在问题，但不能解释问题或完成决定。
3. 后台已经能够返回变化来源、目标设备、两种选择的影响、允许动作、设备关系和决定结果，CLI 适配不需要新增产品规则。
4. CLI 同时服务人和脚本。人类输出应解释设备与后果；JSON 输出必须保持单一、稳定、无提示文字，可直接交给 `jq` 等工具处理。
5. 决定可能在用户查看后、提交前发生变化。提交必须携带变化编号，过期决定只能触发重新读取，不能落到新的变化上。
6. “应用包含本机的移除”和“保留当前设备组”都会带来重大且不易逆转的后果，需要显式确认；脚本必须用明确参数表达同一意图。

## 推荐产品形态

- `uniclip member trust`：读取完整当前状态，不修改任何内容。
- `uniclip member trust decide --change <CHANGE-ID> --choice <apply|keep-current>`：提交当前变化的决定。
- 当应用变化会移除本机时，额外要求 `--confirm-local-removal`；普通确认或 `--yes` 不能代替。
- 交互式决定在提交前展示来源、目标和所选结果，并默认取消。
- `--json` 始终为非交互模式；缺少必要确认参数时直接失败，不读取终端输入。
- 保留 `member removal-status` 作为兼容入口，但明确标记为旧摘要并引导使用 `member trust`。它不能作为新流程的验收依据。

## 不采用的方向

- 让 `member removal-status` 同时承担旧摘要和新决定：名称与输出都无法覆盖完整设备信任问题。
- 直接提供无变化编号的 `accept` / `reject`：可能把旧选择应用到新的待处理变化。
- 在 CLI 根据旧收敛字段推导设备关系或允许动作：会产生第二份业务规则。
- 在 `--json` 模式继续询问确认：会挂住脚本，并污染机器可读输出。
- 用在线、设备数量或到达顺序自动选择结果：违反用户最终决定权。

## 约束

- 用户可见文案遵循现有 CLI 规则，使用英文；PRD 和项目文档使用中文。
- CLI 只负责参数、展示、确认和转交动作，不保存决定队列，不维护设备关系副本。
- 主输出写入标准输出；提示与错误写入标准错误；成功为零退出码，未完成或失败为非零退出码。
- JSON 字段沿用后台稳定结果，不为 CLI 重新定义一套相似但不同的数据模型。
- 设备名称、设备标识和关系信息不得进入日志或额外持久化文件。

## 交付物

- `docs/prd/2026-08-14-cli-device-trust-adaptation.md`

## 参考来源

- `VISION.md`
- `docs/prd/2026-08-13-device-trust-reconciliation.md`
- `apps/cli/AGENTS.md`
- `apps/cli/README.md`
- `apps/cli/src/main.rs`
- `apps/cli/src/commands/member.rs`
- `crates/uc-daemon-contract/src/api/dto/member.rs`
- `crates/uc-daemon-client/src/http/member.rs`
- `crates/uc-webserver/src/api/member.rs`
- Command Line Interface Guidelines：输出通道、JSON、退出码、破坏性确认和非交互行为。
