# 设备组选择 GUI 实施进度

规格：`docs/specs/2026-09-07-device-group-choice-gui-e2e-spec.md`。
本任务不提交、不推送；保留原有 a/b/c/d 和所有无关修改。

- [x] A：确认本机 Engine 与 GUI 驱动，启动隔离窗口。
- [x] B：资料展示、强制选择与结果流程，相关回归测试。
- [x] C：四/五配置成套 userdata 生成、恢复和运行。
- [ ] D：G01–G09 与 U01–U10 实际验证及证据报告。

当前依据：展示功能提交 `0f3a5843`；实际基线与运行 Engine HEAD `fe55e543`，Desktop 有大量前序未提交适配。
GUI 已有 embedded webdriver 及双/三实例工具。不能把默认三实例测试的 skip 当作通过。

## 2026-09-07 简化窗口专项

- 用户确认本机被移除时仅展示“退出设备组 / 保留现有设备”，一句话影响；名单默认收起，点击退出后才二次确认。
- 10 项相关组件/资料测试通过；TypeScript 与实际 Tauri E2E 构建通过。
- 四个真实窗口的简化专项通过：默认未选、名单展开/收起、退出二次确认、返回选择。未提交最终退出。
- 截图：`.cache/device-group-e2e/run-mtql1ng6/screenshots/concise-default.png`、`concise-expanded.png`、`concise-confirm.png`。
- 命令：`CONFLICT_CASE=concise CONFLICT_RUN=.cache/device-group-e2e/run-mtql1ng6/run.json npx --yes --package=node@24 node node_modules/@wdio/cli/bin/wdio.js run e2e/wdio.conflict.conf.mjs`。
- 先前完整移除测试等不到完成视图（90 秒），尚未解决，不计为通过。此前构建早于最终 App 结果展示接入，重跑应先核对构建时点。
- 四配置基线已生成并恢复；五配置加入全部完成，但最终快照遇到 503，基线未保存成功，需完善最终条件等待后重新生成。
- 当前仍未完成 G01–G09 / U01–U10 全矩阵，不能宣布规格完成。
- React Doctor 的全改动扫描含既有界面问题且维护性检查未完整执行，不能宣称全量扫描通过。

## 2026-09-07 后续全套执行

- 使用说明：`docs/guides/device-group-gui-testing.md`；报告：`.planning/device-group-choice-gui/verification.md`。
- 全量 Vitest：176 文件、1119 项通过。原有 Node 测试误用入口及语言包缺项已修正。
- 四/五配置基线均已生成，设为只读；校验和验证，运行副本可写，排除连接/锁文件。
- 工具增加 GUI 串行锁、基线运行租约、迟启动后台清理、PID 对应路径检查、构建来源记录。
- 全套报告 `.cache/device-group-e2e/suite-mtqnzo3i/results.json`：12 轮中 10 轮通过，远端组重启、断线再次选择失败。
- 断线再次选择补齐显式返回核对后，单独复测通过：`.cache/device-group-e2e/run-mtqoiw8w/`。
- G01/G04 增强了实际文本发送与目标 GUI 接收断言，正在复测，不能只用之前的名单通过结果替代。
- 远端组重启后查询持续不可用仍未解决；Engine 未修改。不能宣布整份规格完成。

## 2026-09-07 最终收尾复验

- 最新全量检查：179 个测试文件、1130 项通过；TypeScript 与重新构建的实际 E2E GUI 通过。
- 修复最后一次已完成选择后的查询失败无提示，增加实际窗口的重查恢复断言。
- 查询期限与选择期限分别为具名的 15 秒和 60 秒；真实超时受控案例使用完整期限，不提前伪造超时。
- 补齐资料缺失/null、两种不完整的独立组合、具体等待设备、旧查询迟到、同一窗口逐项衔接、名称更新与影响变化的实际窗口检查。
- 新增 Engine 脱敏事件收集；只保留时间、测试角色、来源、级别和错误分类。调查资料见 `engine-blockers.md`。
- 五配置旧测试漏处理加入之后的独立移除，已补齐各窗口逐项选择；新执行仍复现 A 查询 503，其他四端查询成功且无待处理事项。
- 真实键盘之前有通过记录，但最新完整焦点复验时 Mac 已锁屏。当前工具在发送按键前明确拒绝锁屏环境；U06 不标为全部验收通过。
- 当前最终程序全套运行：`.cache/device-group-e2e/suite-mtqt36x1/results.json`。最终结果以 `verification.md` 为准。
- React Doctor 再次扫描 28 个已改文件：6 个既有动画错误及其他告警，维护性分析未完成、无分数。没有修改无关初始设置界面或压制规则。
- 最终矩阵已结束：13 轮中 10 轮通过、2 轮恢复失败、1 轮锁屏阻挡。最终进程扫描无测试后台残留，运行器退出，租约释放；D 阶段因 G04/G05/U06 未通过而保持未完成。
