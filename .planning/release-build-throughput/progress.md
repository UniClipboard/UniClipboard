# 进度

已确认基线与同期预热作业争用。使用独立worktree，主工作区保持不变。

已实施：依赖基线未变化时跳过主线整套预热；release平台预热max-parallel=1；辅助预热后置。统一矩阵模块，删除每平台setup-matrix，保持SHA、架构、SignPath和原生验收。

第三项证据：最终二进制动作含原生链接，现有Engine关闭target views/seed，MBX有unportable-native-link绕过。未关闭ThinLTO或恢复Cargo target；最终编译提速尚不能宣称完成。

Linux GUI在实际容器内恢复/编译/保存；musl沿用release-sidecar但隔离target和静态flags；新增只编译的受保护预热。覆盖率键本地版本归一化与预热策略保持一致。协议修改会一次性重新预热。

本地：脚本测试311项通过（新增断言后继续复验）；Python快照22项通过/2跳过；预热策略真实Git测试2项通过；actionlint和bash语法通过。Docker socket不可访问且sudo需要密码，容器实测转托管CI。

## PR 跨平台失败修复

PR #2097 首轮 macOS、Windows x64/ARM64 失败均为新增 Linux 场景测试直接读取 `/etc/os-release`，两项测试报 FileNotFoundError；原生导出/导入往返已通过。为这两项测试增加固定 Linux 容器元数据上下文（OS、架构和发行版），不改变生产脚本的真实环境校验。

本地完整 Python 快照测试通过；真实原生 MBX 测试21通过/1未启用插桩测试；额外模拟宿主不存在 os-release，原失败两项均通过。后续核对新提交的三平台远端结果。
