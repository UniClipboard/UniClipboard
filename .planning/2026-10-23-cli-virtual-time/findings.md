# 核对依据

- 当前 HEAD 为 c4e8342e4，初始工作区干净。
- `apps/cli-go/go.mod`：Go 1.26.0，toolchain go1.27.1。
- `send_targets.go` 查询失败返回成功，将权威错误交给最终发送；客户端初始化失败直接退出。
- 查询和 readiness 判断在截止时间检查之前；轮询间隔 500ms，最后一次睡眠截断到剩余预算。
- CLI 局部规则明确禁止单元测试。规格必须明确提出仅对本轮共享等待编排放宽规则，不能沿用交接中“CLI 未必禁止”的假设。本轮不直接修改规则。
- 真实等待测试的主要入口是 `tests/e2e/tests/send_connect_wait.rs`，而非仅 `cli_engine_workflows.rs`；需要逐项迁移映射。
- `apps/cli-go/e2e/scenarios_send.py` 同时承担输出差分与真实传输验收，不能整体删除。
- Go 官方 synctest 文档已核实：测试范围内标准 time 使用虚拟时钟；真实 socket I/O 不属于可判定阻塞，不能包裹现有子进程 E2E。
- 当前不存在 `.github/workflows/ci.yml`、`tests/e2e/README.md`、`internal/ui/spinner.go`；改为搜索实际工作流、`ui.go` 和具体测试文件。
- `send_connect_wait.rs` 共 14 项测试，3 项仅 Unix；计划删除 2 项纯组合三节点用例，保留 12 项真实协作检查。
- 保留测试集的本地配对调整纳入本轮；Python 差分公网夹具保持原状并独立报告。
- `scripts/e2e/build-cli.sh` 存在，daemon 使用 `cargo build -p uc-daemon --features uc-daemon/e2e-rendezvous`，不存在独立 daemon 构建脚本。
- CI 实际入口是 `.github/workflows/pr-check.yml`；`cli-go` 当前仅 vet 和构建，计划必须接入新测试。
- 已授权实施并落地有限测试例外；新测试共 34 个叶子场景。
- 原始/改造后 Go 四组发送差分均一致（86 步/实现），真实公共配对发送场景也通过。
- 真实 E2E 中终端进度测试默认受 UC_E2E_RECORD_ONLY 门控，需要单独标注为未实际执行。
- 第一次基线存在二进制布置错误，已单项复验并启动完整修正基线；不据此计算加速倍数。
- E2E 独立 Cargo.lock 随本地 workspace 包版本由 1.3.0 自动刷新为 1.4.0；未变更依赖约束，最终清理无关锁文件改动。
