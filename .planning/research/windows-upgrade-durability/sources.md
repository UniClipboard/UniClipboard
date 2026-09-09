# 依据与实验记录

日期：2026-09-09。代码依据为 Engine `cc0991e6ab94e5b1368c9e4d9b6421e8d3612da9`；以下代码路径均相对 Engine 仓库。

本文件保留研究阶段的证据。实施后的 Windows 原生结果及修复提交见 [VALIDATION.md](VALIDATION.md)。

## 官方来源

- S1：[Microsoft FlushFileBuffers](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-flushfilebuffers)：要求 GENERIC_WRITE，失败时通过 GetLastError 获取具体原因。
- S2：本机 Rust 1.95.0 源码 `library/std/src/sys/fs/windows.rs:400`：fsync 调用 FlushFileBuffers；datasync 转发 fsync。与 [Rust File 文档](https://doc.rust-lang.org/std/fs/struct.File.html#method.sync_all) 对照。
- S3：[Rust OpenOptions](https://doc.rust-lang.org/std/fs/struct.OpenOptions.html)：选项默认 false；write 与 truncate、create 是独立选择。
- S4：[Microsoft MoveFileExW](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-movefileexw)：REPLACE_EXISTING、WRITE_THROUGH、COPY_ALLOWED 的不同语义。跨卷移动与同卷重命名不能混为一谈。
- S5：[SQLite Atomic Commit](https://www.sqlite.org/atomiccommit.html)：日志、刷新、提交点与故障恢复共同构成原子提交保证。
- S6：[SQLite PRAGMA](https://www.sqlite.org/pragma.html)：synchronous、journal_mode 与 wal_checkpoint 的持久性和返回状态；普通文件刷新不能替代数据库协议。
- S7：[Microsoft CreateFileW](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilew)：访问权限、共享权限、OPEN_EXISTING 与目录打开规则。
- S8：[Rust Windows OpenOptionsExt](https://doc.rust-lang.org/std/os/windows/fs/trait.OpenOptionsExt.html)：默认共享标志为 FILE_SHARE_READ、FILE_SHARE_WRITE、FILE_SHARE_DELETE。

## 已确认的普通文件调用

| 位置 | 用途 |
| --- | --- |
| `crates/uc-infra/src/security/profile_storage_upgrade/target.rs:438` | 拆分数据库后刷新 |
| `crates/uc-infra/src/security/profile_storage_upgrade/primary_payloads.rs:155` | 正文转换数据库复制后刷新 |
| `crates/uc-infra/src/security/profile_storage_upgrade/primary_payloads.rs:253` | 保留不可读 blob 的原始密文后刷新 |
| `crates/uc-infra/src/security/profile_storage_upgrade/primary_payloads.rs:517` | 整理正文数据库后刷新 |
| `crates/uc-infra/src/security/profile_storage_upgrade/derived_payloads.rs:811` | 复制派生内容目录中的文件后刷新 |
| `crates/uc-infra/src/security/space_control_generation/persistence.rs:79` | 整理控制数据库后刷新 |
| `crates/uc-infra/src/security/space_control_generation/persistence.rs:103` | 可写控制数据库检查点后刷新 |

这些调用都是以 File::open 获得普通文件句柄后 sync_all。对 uc-infra/src 下全部 sync_all 调用进行了检索，并区分了本来持有可写句柄的写入和仅在非 Windows 编译的目录刷新。

已有正确参考：

- `profile_storage_upgrade/persistence.rs:156`：create_new + write，写入密文进度后刷新原句柄，再替换并处理父目录。
- `profile_storage_upgrade/target.rs:529`：候选快照采用写入临时文件、刷新、关闭、替换的顺序。
- `fs/atomic_publish.rs`：已有平台原生发布能力，但契约是不覆盖目标，不能直接替代升级进度的覆盖更新。
- `docs/exec-plans/completed/033-immutable-content-protection-context.md`：记录来源快照、generation、摘要和升级阶段的设计。
- `.github/workflows/pr-check.yml`：现有两个检查任务均使用 macos-14，没有 Windows 原生存储测试任务。

## 实验

### 中途失败与重启

在独立 Engine 诊断副本执行合成实验 `research_partial_separation_blocks_restart`：

1. 创建合成 SQLite 与一行控制数据。
2. 调用真实升级器，推进到 TargetStaged。
3. 仅修改 profile 候选中的控制行，模拟拆分已改写、刷新失败而进度未推进的磁盘状态。
4. 保持原始数据库修订不变，销毁升级器，再创建新实例重试。
5. 现有实现返回 Corrupt。

实际输出：

```text
partial_separation_restart_rejected=true source_revision_unchanged=true
test result: ok. 1 passed
```

这里的测试通过表示成功复现现有缺陷，不表示恢复已经修复。该故障回放没有模拟 Windows 系统 API，也没有模拟突然断电。

### 文件刷新探针

附带 `flush-probe.rs`，只使用标准库，在系统临时目录创建合成文件，比较只读刷新和可写刷新，并检查字节未改变。探针不会读取用户资料。

Mac 编译与运行：

```bash
rustc --edition 2021 .planning/research/windows-upgrade-durability/flush-probe.rs -o /tmp/uc-windows-flush-probe
/tmp/uc-windows-flush-probe
```

Windows 原生执行：

```powershell
rustc --edition 2021 .planning/research/windows-upgrade-durability/flush-probe.rs -o "$env:TEMP\uc-windows-flush-probe.exe"
& "$env:TEMP\uc-windows-flush-probe.exe"
```

Windows 预期为只读刷新返回拒绝访问、可写刷新成功。原生执行尚未完成；不把 Mac 结果或文档结论记成 Windows 实测结果。

Mac 实际运行结果：

```text
platform=macos
readonly_sync_ok=true
writable_sync_ok=true contents_unchanged=true
```
