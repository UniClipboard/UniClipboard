# 进度记录：开发者手动选择网卡配对工具

## 2026-05-13

- 开始实现开发者手动选择网卡配对工具。
- 使用 planning-with-files 管理计划。
- 使用 test-driven-development，准备先写失败测试。
- 当前分支 `agate-surgeon` 已跟踪 `origin/agate-surgeon`，开始前工作区干净。
- 读取了本仓库 workflow / Rust / architecture / src-tauri / uc-cli 规则，确认新增命令必须通过应用层 facade，不在 CLI 直接访问底层实现。
- planning-with-files 的 `session-catchup.py` 在当前技能安装目录不存在；已使用现有 `task_plan.md` / `findings.md` / `progress.md` 继续。
- 已先写红灯测试：
  - CLI 解析 `uniclip dev pairing addrs` 和 `uniclip dev pairing issue --addr <IP>`。
  - infra 限定 ticket 只保留选定 IP、选定 IP 不存在时报错、地址列表复用 ticket 过滤规则。
  - application 用例按选定 IP 签发后仍把邀请放入 holder。
- 红灯结果已确认：
  - `cargo test -p uc-cli dev_pairing_manual_address_commands_parse` 失败，因为 `dev pairing` 命令不存在。
  - `cargo test -p uc-application selected_address_path_calls_selected_port_and_parks_aggregate` 失败，因为 selected-address port/usecase 方法不存在。
  - `cargo test -p uc-infra serialize_ticket_for_selected_ip_keeps_only_that_ip` 失败，因为 selected-ticket helper、地址不可用错误和候选地址 helper 不存在。
- 实现后 `cargo test -p uc-application selected_address_path_calls_selected_port_and_parks_aggregate` 已通过。
- 一次 infra 测试命令把多个过滤名直接传给 `cargo test`，Cargo 报 `unexpected argument`；改用单一过滤条件重跑。
- `cargo test -p uc-infra rendezvous::invitation_adapter` 已通过，18 个 rendezvous invitation 测试全绿。
- `cargo test -p uc-cli dev_pairing_manual_address_commands_parse` 已通过。
- `cargo test -p uc-application usecases::pairing::issue_invitation` 已通过，6 个 issue invitation 测试全绿。
- 按端口规则把“列出配对邀请地址”拆成独立查询端口后，重新跑以上三组测试均通过。
- 完整验证继续通过：
  - `cargo test -p uc-cli`：32 个测试通过。
  - `cargo run -p uc-cli -- --help`：主 help 未展示隐藏 `dev` 入口。
  - `cargo run -p uc-cli -- dev pairing --help`：展示 `addrs` / `issue`。
  - `cargo run -p uc-cli -- dev pairing issue --help`：展示必需的 `--addr <ADDR>`。
  - `cargo run -p uc-cli -- --profile codex-manual-nic-smoke --dev --json dev pairing addrs`：实际输出当前候选地址 JSON。
  - `cargo test -p uc-bootstrap --tests --no-run`：bootstrap 相关测试二进制编译通过。
  - `cargo test -p uc-application facade::space_setup`：21 个 facade 测试通过。
