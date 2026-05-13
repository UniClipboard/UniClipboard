# 进度记录：Only LAN + 允许虚拟网卡配对失败

## 2026-05-13

- 开始修复任务。
- 使用 planning-with-files 管理计划。
- 使用 test-driven-development，准备先写失败测试。
- catchup 脚本路径不存在，已记录并改用当前 git 状态和文件检查继续。
- 接续已有诊断，确认代码尚未修改，当前只新增了 `task_plan.md`、`findings.md`、`progress.md`。
- 已读取项目 Rust 规则：Rust 命令必须从 `src-tauri/` 执行，生产代码使用 `tracing` 记录错误。
- 完成阶段 1：确认地址过滤规则已存在但只在 `node.rs` 内部使用，配对码生成没有复用。
- 进入阶段 2：准备先补 sponsor ticket 地址过滤的失败测试。
- 已补 sponsor ticket 过滤测试；当前红灯结果符合预期：缺少 `serialize_filtered_endpoint_ticket`，说明配对码生成路径还没有可复用的过滤入口。
- 实现共享地址过滤和配对拨号重试后首次测试编译失败；已修正连接 helper 可见范围和 node 测试导入。
- sponsor ticket 过滤测试已通过。
- 已运行 `cargo fmt`。
- `cargo test -p uc-infra rendezvous::invitation_adapter` 通过：15 个测试全部通过。
- `cargo test -p uc-infra pairing::session` 通过：8 个测试全部通过。
- `cargo test -p uc-infra network::iroh::node::tests::addr_filter` 通过：3 个测试全部通过。
- `cargo check -p uc-infra` 通过。
- 收窄内部接口后重新运行 sponsor ticket 过滤测试，通过。
- 收窄内部接口后重新运行 `cargo check -p uc-infra`，通过。
- 最终运行 `cargo test -p uc-infra`，通过：lib 单测 288 通过、1 忽略；集成测试 12 通过；doc tests 16 通过、3 忽略。
- `git diff --check` 通过。
