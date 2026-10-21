已恢复上轮内存证据，加载Desktop与Engine约束，准备独立Engine工作树及压缩对照。
Engine单线程压缩已在独立分支提交，79个受影响crate测试、全workspace all-targets、格式与架构检查通过。原二进制arena2对照10/10完成，静置PSS65.72MiB（原126.93MiB），入库中位数0.191s（原0.267s）；不能仅据10样本保证所有并发吞吐。
已在Linux/glibc入口增加mallopt arena上限2，显式MALLOC_ARENA_MAX与GLIBC_TUNABLES覆盖优先。Desktop原配置check通过。升级固定Engine提交时，offline Cargo重新解析遇到未缓存macOS core-foundation版本，正在用正常在线解析保持锁文件一致。

全部完成：最终去符号优化构建100张图片/120秒静置，最后15秒PSS58.53MiB；daemon45测试、observability22测试、Desktop workspace all-targets、fmt、Engine边界与diff检查均通过。原始证据与构建指纹保存在本机既有诊断目录。

2026-10-21：用户授权提交PR。Desktop分支已获确认改名为perf/daemon-memory；Engine提交f56aca62fe8880607b84772a89a7a3ec450a30a0已推送，对应 https://github.com/UniClipboard/Engine/pull/229 。安装版未替换。正在对优化后二进制补采堆数据，研究缓存、SQLite、网络与分配器的进一步空间。

后续研究完成：Desktop https://github.com/UniClipboard/UniClipboard/pull/2055 已创建，两PR均无合并冲突。补采10/10图片及120秒静置，优化版存活堆17.32MiB；主要为剪贴板4.30、HTTP/OpenAPI4.29、网络4.04、SQLite3.33MiB。已恢复匹配符号，模块汇总守恒、官方峰值147,965,872字节一致，火焰图已生成。研究发现OpenAPI深拷贝、Wayland容量余量与网络固定UDP缓冲，建议后续独立验证，未扩大当前生产改动。新样本启用Heaptrack，其PSS不用于替换无分析器100张验收结果。
