# 设备组选择的本机多配置测试

## 范围

本工具在 macOS 同时运行四或五个真实 Tauri 窗口及对应后台。使用真实 Engine 和加密资料；没有 Windows 或移动设备验收。
场景依据见[规格](../specs/2026-09-07-device-group-choice-gui-e2e-spec.md)。全部验收状态见[当前报告](../../.planning/device-group-choice-gui/verification.md)，不能把工具能够启动等同于所有场景通过。

## 准备

从 Desktop 仓库根目录执行。先确认 Cargo 的 `uc-engine` 与配套依赖解析到要测试的 Engine 源码，再构建后台和专用 GUI：

```sh
cargo metadata --locked --format-version 1
bun run daemon:dev
bun run tauri build --debug --no-bundle --features e2e --config src-tauri/tauri.e2e.conf.json
```

Cargo 串行运行，沿用已有外置 target 和编译缓存。界面或原生代码变化后重新构建；不要用旧程序验收新源码。
测试使用现有 embedded WebDriver。Node 24 用于规避本机默认 Node 加载测试依赖的问题。

## 生成与复用资料

```sh
node e2e/conflict-userdata.mjs generate four
node e2e/conflict-userdata.mjs generate five
```

生成器通过正常创建、邀请和加入流程形成同一设备组；成员数量和激活状态达到条件后才保存。
基线位于 `.cache/device-group-e2e/four/`、`.cache/device-group-e2e/five/`，已经存在时不会隐式覆盖。
清单包括 Engine 提交、角色、前置状态、校验值及仅供本机测试的凭据。基线目录只读，整个测试目录仅供当前用户访问，不提交这些文件。

生成时必须等到所有成员关系为已确认且可用。需要重新生成时先停止相关运行，再显式执行
`node e2e/conflict-userdata.mjs archive four`（或 `five`）归档旧基线；归档保留原数据，不删除或覆盖。

恢复会整套复制持久化资料（包括解锁所需资料），排除后台端口、进程号、锁和会话连接文件。新配置统一使用
`conflict-e2e-<运行编号>-<角色>`，不会写入或重置原有 a/b/c/d。
每次都创建新的运行副本，失败副本和截图保留，因此可以回查当次现场；“恢复起点”是再次从原基线创建副本，不是覆盖失败现场。

基线版本或校验值不匹配会停止。Engine 变化后应明确验证迁移或生成新的基线；不要手动修改校验值放过不兼容资料。
同基线只允许一个运行副本在线，GUI 套件也串行运行，避免相同身份副本和自动化端口相互干扰。

## 运行场景

下列入口自动恢复基线、启动实际窗口、运行操作、保存证据并停止本轮后台：

```sh
npx --yes --package=node@24 node e2e/conflict-suite.mjs four apply
npx --yes --package=node@24 node e2e/conflict-suite.mjs four cross-local
npx --yes --package=node@24 node e2e/conflict-suite.mjs four cross-remote
npx --yes --package=node@24 node e2e/conflict-suite.mjs four disagreement
npx --yes --package=node@24 node e2e/conflict-suite.mjs five local-remove
npx --yes --package=node@24 node e2e/conflict-suite.mjs four new-peer
npx --yes --package=node@24 node e2e/conflict-suite.mjs four disconnect
npx --yes --package=node@24 node e2e/conflict-suite.mjs four restart-cycle
npx --yes --package=node@24 node e2e/conflict-suite.mjs four response-loss
npx --yes --package=node@24 node e2e/conflict-suite.mjs four controlled
npx --yes --package=node@24 node e2e/conflict-suite.mjs four native-keyboard
```

| 场景                       | 验证内容                                                                                                      |
| -------------------------- | ------------------------------------------------------------------------------------------------------------- |
| apply                      | GUI 接受移除、结果关闭、保留设备实际收到测试文本、已移除关系的发送结果                                        |
| cross-local / cross-remote | 分别离线移除不同目标，恢复连接后选择本地/远端分支；后续重启稳定性                                             |
| disagreement               | 对同一次移除分别接受和保留，验证实际成员差异及保留组同步                                                      |
| local-remove               | 本机移除的二次确认、实际退出与结果反馈                                                                        |
| new-peer                   | 四设备基线加一台全新设备，远端候选出现本地未见成员及名称；逐项处理加入和移除，再检查真实文本同步              |
| disconnect                 | 断开本机界面与后台的真实连接并阻断查询，期间另一台设备完成真实选择，恢复后自动补查                            |
| restart-cycle              | 未处理、处理、已处理三个阶段之间真正退出 GUI 和后台，再重新启动                                               |
| response-loss              | 后台真实执行，客户端丢失回复，再查询结果，验证不重复提交                                                      |
| controlled                 | 在真实窗口的查询边界注入展示/故障样本，验证旧资料、长名称、中英文、主题、窄窗口、返回结果和请求超时           |
| native-keyboard            | 单个真实测试窗口，使用系统按键验证 Esc、空格、Tab、回车及焦点；确认收到的按键是系统事件，不用脚本伪造键盘事件 |

`new-peer` 使用 `four` 基线，但实际启动五个配置；第五个配置是全新身份。
`native-keyboard` 只启动一扇窗口，要求 Mac 已解锁、当前运行终端有控制 macOS System Events 的权限。
锁屏或无法确认交互会话时，会在复制资料和发送按键前明确停止；每次发送系统按键前也会重新检查。不自动跳过，不尝试解锁或绕过权限。
检查要求活动控制台已经完成登录；macOS 在已解锁会话中可能省略锁屏标志，不能据此判为锁屏。明确锁屏、未完成登录或异常字段类型仍会拒绝发送按键。
受控样本不是跨配置业务完成证明。其注入逻辑仅在测试脚本中，不进入生产代码；真实场景的最后选择仍由 GUI 执行。

运行全部场景（包含四/五配置的重复恢复运行）：

```sh
npx --yes --package=node@24 node e2e/conflict-all.mjs
```

也可使用 `bun run e2e:conflicts four apply` 与 `bun run e2e:conflicts:all`。

全部运行器不会因为某项失败而跳过后续独立项，最终有任何失败时返回非零状态。
每项控制台输出 `Run manifest`，对应 `.cache/device-group-e2e/run-<编号>/run.json`。
完整运行报告保存在 `.cache/device-group-e2e/suite-<编号>/results.json`，包含每项退出结果、控制台记录及运行清单位置。

## 手动恢复与停止

### 恢复旧故障现场

修复 Engine 后可显式从此前失败运行复制新配置，验证旧资料迁移与恢复，而非只验证新建组：

```sh
npx --yes --package=node@24 node e2e/conflict-suite.mjs four recover-frozen .cache/device-group-e2e/run-<旧运行编号>/run.json
```

入口支持此前四配置远端组故障，以及 `four new-peer` 形成的五配置故障。它在新窗口处理未完成选择，检查成员名单、实际文本接收和被排除目标的发送结果。
只允许本工具管理的四/五配置清单；原资料复制前后校验值必须一致，拒绝存在非空 WAL 的不完整快照。新清单记录来源与校验值，原失败现场不变。
此入口明确允许旧 Engine 提交，用于迁移验证；普通基线恢复仍检查版本，不要改动基线清单来绕过检查。

### 普通恢复

单独恢复用于现场调查：

```sh
node e2e/conflict-userdata.mjs restore four
```

将该命令输出的清单路径作为 `CONFLICT_RUN` 传给 `e2e/wdio.conflict.conf.mjs`；优先使用上面的场景入口，它负责完整清理。
停止时显式提供该次清单，下面的 `<运行编号>` 应替换为实际值：

```sh
node e2e/conflict-userdata.mjs stop four .cache/device-group-e2e/run-<运行编号>/run.json
```

不要直接删除互斥目录来绕过正在运行的检查。工具会校验测试配置及进程归属，并处理 GUI 结束后延迟启动的后台。
停止不会删除基线、运行数据和证据。若需要清理磁盘，先停止对应运行，再单独确认具体可清理路径。
手动操作测试窗口时应先关闭对应 GUI，再执行停止命令；自动场景入口会完成窗口退出及后台清理。

## 证据边界

- `screenshots/` 保存选择前、选中、结果和边界样本；失败时尝试保存各窗口现场。
- macOS 后台 WebView 会暂停动画，截图工具把有限动画推进至最终状态；不改布局、不合成业务内容，不将加载中截图冒充完成。
- `result-<场景>.json` 只在该场景断言全部通过后写入。缺少该文件或退出码非零，不能记作通过。
- `build.json` 记录 Engine/产品提交、工作区状态、实际程序路径与校验值。失败诊断只保存状态分类和数量；选择前后对照使用测试角色与本轮临时事项编号，不输出设备身份、名称或完整响应。
- `engine-events.jsonl` 按时间合并本轮各配置的 Engine 事件，只保留时间、角色、级别、来源及结构化错误分类，不复制原始日志正文。`engine-evidence.json` 明确记录无法读取的角色，不能把缺失日志算作无错误。
- 断线场景通过专用 E2E 桥接关闭真实后台 WebSocket，并暂时阻断成员查询；恢复后依靠重连补查。该控制入口不出现在普通构建的可执行脚本中。
- 正常的短暂 503 查询在有限时间内等待；用户决定的请求不因此自动重发。`state_changed` 会让测试像用户一样重新查看并显式选择。
- 启动、候选出现、选择完成、关系恢复都有有限等待。超时保留失败，不能增大超时掩盖缺失状态。
- 测试资料包含专用身份和测试凭据，不能提交或直接公开打包；对外分享只提供脱敏结果和使用专用名称的截图。
