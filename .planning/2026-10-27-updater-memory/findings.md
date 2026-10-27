# 发现
- 当前 Download 以 append 聚合完整字节，host_updater.bytes 长期保留。
- Linux AppImage、Windows ZIP 解包同样全量读入内存；macOS 解包待核对。
- minisign v0.3.0 Reader 支持流式预哈希验签；旧 Ed（非预哈希）签名不能用 Reader 验证，需明确拒绝并覆盖真实分发签名。
- 当前包类型只识别 dpkg/rpm；pacman 被归为 unknown。调度器未检查自更新能力。
- 当前实际进程无法读取 /proc/PID/mem，尚未用堆剖析证实具体内存归属；本任务修复的是已确认的整包内存设计缺陷。
- 兼容决定：旧发布签名目前由 minisign.Sign 生成（Ed，覆盖原始消息）。使用只读文件映射验证旧签名并立即解除映射；新签名工具改为 Reader.Sign（ED，预哈希），旧客户端也能读取 ED。移除旧签名路径的条件已写入代码：所有受支持 feed 更新为 ED sidecar。
- 文件产物通过 WithFile 固定生命周期；Close 等待正在使用的文件。宿主退出取消并等待下载，再删除文件。安装操作串行化。
- Engine 遥测安装类型暂不含 pacman；desktop DTO 与前端支持 pacman，遥测适配边界临时映射 Unknown，并附 Engine TODO。
- 生成客户端首次在 schema 生成完成前运行，待 schema 完成后必须重新生成，不能采用首次结果。
