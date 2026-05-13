# 任务计划：修复 Only LAN + 允许虚拟网卡配对失败

## 完成标准

- 生成配对码时，不再把本机不可用于跨设备连接的虚拟地址放进 sponsor ticket。
- Only LAN + 允许虚拟网卡时，Tailscale `100.64.0.0/10` 和 `fd7a:115c:a1e0::/48` 地址可以保留。
- Fedora 加入配对时使用带超时和路径日志的连接流程，能记录实际选中的连接路径。
- 新测试先能暴露当前问题，再在修复后通过。
- 从 `src-tauri/` 跑过相关 Rust 测试。

## 阶段

| 阶段 | 状态 | 内容 |
| --- | --- | --- |
| 1 | complete | 建立计划文件，复核失败证据和代码入口 |
| 2 | complete | 先写失败测试，覆盖 sponsor ticket 地址过滤和配对拨号路径 |
| 3 | complete | 实现地址过滤和配对拨号修复 |
| 4 | complete | 运行目标测试并修正问题 |
| 5 | complete | 汇总结果 |

## 错误记录

| 错误 | 尝试 | 处理 |
| --- | --- | --- |
| `session-catchup.py` 不存在 | 使用 skill 文档中的脚本路径执行 catchup | 已确认该 skill 目录没有 scripts 子目录，改用手动检查当前计划文件和 git 状态 |
| 首次绿灯编译失败 | 运行 sponsor ticket 过滤测试 | `connect_with_staggered_retry` 可见范围仍是模块内部，且 node 测试少了 `IpAddr` 导入；已改为 crate 内可见并补导入 |
