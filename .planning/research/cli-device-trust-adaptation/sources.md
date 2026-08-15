# 调研来源：CLI 设备信任核对适配

## 已确认产品规则

- `VISION.md`：CLI 是正式交付形态；各平台使用同一套设备身份、信任和传输规则；产品没有账号、管理员或固定主设备。
- `docs/prd/2026-08-13-device-trust-reconciliation.md`：定义设备信任变化、两种用户选择、本机退出、不同空间、版本过旧、资料无法验证及完整当前结果。
- `.planning/research/device-membership-reconciliation-prd/BRIEF.md`：确认产品应围绕设备与选择后果表达，不展示内部成员收敛过程。

## 当前 CLI 证据

- `apps/cli/src/main.rs`：`member` 当前只有 `remove` 和 `removal-status` 两个子命令。
- `apps/cli/src/commands/member.rs`：默认输出旧成员收敛阶段、事件数量、待决定数量、不同空间数量和需更新数量，不能完成用户决定。
- `apps/cli/src/commands/status.rs`：总体状态只展示旧收敛阶段。
- `apps/cli/README.md`：公开命令说明中没有设备信任查询或决定入口。
- `apps/cli/AGENTS.md`：CLI 只负责参数、终端输出、交互输入和退出结果；不得重新实现业务规则；人类输出和 JSON 输出必须同时支持。

## 当前后台能力

- `crates/uc-daemon-contract/src/api/dto/member.rs`：完整结果包含变化编号、来源、目标、两种选择的影响、允许选择、设备关系、不可用原因和决定结果。
- `crates/uc-webserver/src/api/member.rs`：后台已经提供设备信任查询和决定入口。
- `crates/uc-daemon-client/src/http/member.rs`：CLI 使用的客户端目前只接入旧收敛查询，尚未接入完整设备信任能力。
- `crates/uc-daemon-client/src/service.rs`：CLI 依赖的统一服务目前只暴露主动移除和旧收敛查询。

## 现有 CLI 交互约定

- `apps/cli/src/commands/join.rs`：破坏性空间切换默认要求确认，非交互环境可通过明确参数跳过普通确认。
- `apps/cli/src/commands/mobile_sync/setup.rs`：JSON 模式等同于非交互模式，缺少必要确认时直接失败，不显示终端提示。
- `apps/cli/src/output.rs`：机器可读结果输出到标准输出，序列化失败按命令错误处理。
- `apps/cli/src/exit_codes.rs`：退出数值集中管理，不在命令中散落定义。

## 外部通用规范

- Command Line Interface Guidelines，https://clig.dev/：
  - 成功使用零退出码，失败使用非零退出码。
  - 主结果和机器可读内容进入标准输出，提示与错误进入标准错误。
  - 复杂结果应提供 JSON，供脚本稳定处理。
  - 状态修改后应告诉用户实际发生了什么。
  - 中高风险的破坏性操作应要求确认；严重操作应要求难以误触的明确确认，同时保留可脚本化的显式参数。
  - 非交互输出不应包含动画；供脚本使用的输出格式应保持稳定。

## 调研限制

浏览器自动化环境当时没有可用页面，因此未引用需要登录或无法直接获取的产品界面。外部通用规范通过其公开页面读取；产品行为结论以仓库内已经确认的 PRD 和当前代码为准。
