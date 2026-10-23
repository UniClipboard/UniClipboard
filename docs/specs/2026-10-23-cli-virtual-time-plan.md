# CLI 虚拟时间测试实施计划

## 状态与入口

状态：实现与发送范围验证已完成，全量兼容验收存在已记录例外。进度与实测结果见[验证记录](2026-10-23-cli-virtual-time-verification.md)。

行为、范围、V01–V15 和真实 E2E 映射以[规格](2026-10-23-cli-virtual-time-spec.md)为唯一来源，本文只定义实施顺序、文件责任与完成门槛。任何测试删减须先满足规格中的承接条件。

## 第一步：确认测试规则与建立基线

目标：明确允许测试的边界，并取得可比较的真实等待成本。

涉及文件：`apps/cli-go/AGENTS.md`、`apps/cli-go/e2e/README.md`。

1. 将规格中的有限测试例外落到 CLI 规则，保留真实 daemon 和差分 E2E 义务，不能把其他模块泛化为允许单元测试。
2. 记录当前 HEAD、工作区变化、Go 版本、机器、操作系统及构建模式。不要覆盖已有用户改动。
3. 构建当前 daemon 和 Go CLI，清点 `send_connect_wait` 在本平台的测试数量，运行原测试集，保留各测试及配对准备耗时。
4. 核实 `LocalRendezvous` 所需的 daemon 测试 feature 已启用，使用现有构建脚本，不新增 production 测试路由。
5. 性能证据写到 `target/cli-virtual-time/`，最终将脱敏摘要写入本任务的验证记录。原始输出遵循仓库日志与凭据规则。

完成门槛：规则已明确，基线包含实际结果；公网准备失败应标明失败阶段与已用时间，不能宣称得到完整性能基线。

## 第二步：提取设备等待用例的共享生产入口

目标：让在线、离线、查询失败和截止时间场景运行生产算法。

涉及文件：`apps/cli-go/internal/commands/send_targets.go`、新增相邻的 `send_targets_test.go`。

1. 保留 `targetReadiness` 为唯一目标条件判断，不复制 DTO 或规则。
2. 将 `/peers` 查询做成包内函数参数；客户端发现与创建留在外层，并保留发现失败的原退出路径。
3. 提取共享轮询函数，输入绝对截止时间与等待通知；返回足够渲染原输出的最小结果。输出适配层继续管理 spinner 生命周期。
4. 删除被提取的旧循环，生产 `waitForSendTargets` 调用唯一新入口。不增加运行开关、通用 Clock 或全局可替换客户端。
5. 添加 V01–V09、V12、V13 的等待部分。使用虚拟经过时间提供状态；边界测试直接控制每轮快照，不依赖同时唤醒的协程抢占顺序。
6. 精确断言虚拟耗时、查询次数或关键查询时刻、等待原因及退出结果；避免脆弱的真实墙钟下界断言。

完成门槛：60 秒离线场景通过且没有真实等待；500ms 轮询截断及先查询后判截止时间均有覆盖。查询失败不得被误改成直接返回错误。

## 第三步：验证准备预算与单次发送

目标：证明生产命令共享预算、不会因轮询重复发送。

涉及文件：`apps/cli-go/internal/commands/send.go`、新增 `send_dispatch_test.go`。

1. 在现有调用边界提取最小发送准入编排，生产和测试均调用它；生产发送类型选择继续调用现有实现。
2. 在准备 I/O 边界以包内函数参数替代真实 daemon 启动与租约获取；不改变真实适配器或 daemon API。
3. 在 `synctest.Test` 内生成 deadline，令前置准备消耗虚拟 57 秒，观察目标等待只使用余下 3 秒，同时断言各阶段接收到原 deadline。
4. 以生产编排接收的发送操作统计调用数，补全 V01–V04、V09–V11、V14、V15；失败、取消零调用，就绪或查询失败最多一次，发送失败不重试。
5. 覆盖生产计算等待开关的路径，避免测试绕过 `timeout=0` / resend 分支。租约释放使用可观测的窄替身，保持原所有权语义。
6. 输出与真实信号行为留给现有 E2E，测试不得用真实 os.Signal 订阅、socket 或子进程填入虚拟时间测试范围。

完成门槛：V01–V15 全部有对应断言，测试没有重写发送条件；生产调用关系能够清楚展示“输入解析 → 一个 deadline → 准备 → 一次所选发送”。

## 第四步：保留真实协作并迁移重复组合

目标：缩减昂贵组合准备，同时保留真实进程、协议和内容结果证据。

涉及文件：`tests/e2e/tests/send_connect_wait.rs`，以及确有需要时的 `tests/e2e/src/pairing.rs`。

1. 复用 `LocalRendezvous` 和 `TestDaemon` 的现有配置入口，给本套保留的配对场景注入本地目录地址；可配置 pairing helper 必须由既有 helper 委托或显式调用，不复制配对流程。
2. 为两个或三个节点共享并持有同一个服务对象，检查重启后地址仍保留；不让服务在返回夹具时被提前释放。
3. 运行完整原场景验证夹具迁移，成功后按规格删除两项已被承接的三节点 E2E，并更新源文件中的测试说明。
4. 保留 `one_connected_device_is_enough_without_the_peer_flag` 的真实广播与结果验证，保留真实重连单份内容、离线后重启无内容、取消、自动启动和文件场景。
5. 本轮不任意缩短重连/送达观察窗口或删掉 settle 等待。若后续要改为事件观察，必须有独立等价性证据。
6. 更新测试 README：区分虚拟时间组、本地目录真实组、仍需公网的 Python 差分组和公网服务冒烟。公网冒烟独立触发，不混进本地组耗时。

完成门槛：本平台保留场景全部通过；删除列表与规格一致，每一项都有接管测试。失败必须区分配对、重连、CLI 等待和传输阶段，不能把未查明故障归因于公网。

## 第五步：接入 CI 并交付性能证据

目标：让提速和覆盖在后续改动中持续成立。

涉及文件：`.github/workflows/pr-check.yml`、CLI 测试说明，新增本规格同目录的验证记录。

1. 在现有 `cli-go` 作业中加入新测试，复用现有 Go 版本配置。先运行一次再运行竞争检测；不得只保留构建检查。
2. 用预编译测试二进制测量执行耗时，连续 20 轮单独调用，记录每轮结果与墙钟耗时。
3. 用相同条件重新测量保留的真实等待组，分别列出纯等待组合移除收益、本地配对变化和剩余真实传输成本；不能把两种收益混成虚拟时钟效果。
4. 跑 CLI 规则要求的生成一致性、vet、构建、help 与差分检查；记录所用 Rust 开发 CLI 和 daemon 二进制版本。
5. 填写验证记录：V 编号对应测试名、E2E 保留/删除对应关系、数量、耗时、20 轮与竞争检测结果、未完成项与原因。实测结果之前不预填“通过”或加速倍数。

完成门槛：满足规格第 7 节；任何必做检查未通过就保留未完成状态。

## 验证命令

以下从仓库根执行，测试名对应已实现的入口。任何手动 CLI 操作继续经 `e2e/iso.py` 或 `e2e/compat.py`，不使用真实 HOME。

### Go 测试与重复性

```bash
(cd apps/cli-go && go test -count=1 -run '^TestSendVirtualTime' ./internal/commands)
(cd apps/cli-go && go test -race -count=1 -run '^TestSendVirtualTime' ./internal/commands)
mkdir -p target/cli-virtual-time
(cd apps/cli-go && go test -c -o ../../target/cli-virtual-time/commands.test ./internal/commands)
python3 - <<'PY'
import json
import pathlib
import subprocess
import time

out = pathlib.Path('target/cli-virtual-time')
rows = []
for round_number in range(1, 21):
    start = time.perf_counter()
    result = subprocess.run(
        [str(out / 'commands.test'), '-test.v', '-test.count=1',
         '-test.run=^TestSendVirtualTime'],
        capture_output=True, text=True,
    )
    elapsed = time.perf_counter() - start
    (out / f'round-{round_number:02d}.log').write_text(
        result.stdout + result.stderr, encoding='utf-8',
    )
    rows.append({'round': round_number, 'seconds': elapsed,
                 'exit_code': result.returncode})
(out / 'timings.json').write_text(json.dumps(rows, indent=2), encoding='utf-8')
assert all(r['exit_code'] == 0 for r in rows), rows
assert all(r['seconds'] < 1 for r in rows), rows
PY
```

执行前先从 `go test -list '^TestSendVirtualTime'` 及详细输出确认用例实际存在，防止零用例成功。计时使用外部墙钟，避免在虚拟时钟内测量实际耗时。上述二进制路径用于 Linux/macOS；Windows 使用 `.exe` 后缀，Unix 信号与伪终端测试按平台编译条件记录。

### 真实 E2E

```bash
cargo build -p uc-daemon --features uc-daemon/e2e-rendezvous
scripts/e2e/build-cli.sh
cargo test --manifest-path tests/e2e/Cargo.toml --test send_connect_wait -- --list
cargo test --manifest-path tests/e2e/Cargo.toml --test send_connect_wait -- --ignored --test-threads=1
```

实施前核对构建参数及脚本当前约定；daemon 必须包含现有 `e2e-rendezvous` 支持。基线与改造后保持同一线程数，CI 仍沿用其既有全套 E2E 调度方式。

### CLI 兼容性

```bash
(cd packages/desktop-host-go && go generate ./buildinfo && git diff --exit-code -- buildinfo)
(cd apps/cli-go && go vet ./...)
(cd apps/cli-go && go build -o ../../target/compat/go/uniclip ./cmd/uniclip)
python3 apps/cli-go/e2e/dump_help.py target/compat/go/uniclip target/compat/help-go
python3 apps/cli-go/e2e/compat.py --rust target/compat/rust --go target/compat/go --out target/compat/run
```

两个 compat 目录均须准备同版本的 daemon；不能仅将 Go CLI 放入目录后就视为具备执行条件。完整兼容检查仍可能访问公网，结果须单独报告。

## 建议提交边界

实施时按单一意图组织，当前不创建提交：

1. `docs: define CLI virtual-time testing scope`：测试约定及规格。
2. `refactor: isolate send target polling dependencies`：共享等待入口及其行为保持测试。
3. `refactor: isolate send dispatch preparation`：最小发送准入编排及预算、调用次数测试。
4. `test: use local rendezvous for send wait scenarios`：本套 E2E 夹具配置。
5. `test: migrate redundant send wait combinations`：覆盖生效后删除两项重复组合。
6. `test: gate CLI virtual-time coverage in CI`：持续集成与验证记录。

每步都应可独立构建和验证；若需要额外修复或行为变化，独立说明并重新判断范围，不能夹带到无行为变化的提取中。
