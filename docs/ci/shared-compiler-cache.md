# Engine 共享编译缓存

## 入口与所有权

macOS/Linux sidecar、musl CLI daemon、Rust PR/E2E、coverage、warmup 和 Linux 独立打包使用 Engine 的 MBX 1.18.0 实现。`scripts/build-cache/engine-tools.sh` 从 Engine 自动化提交 `2f9afc9956b0cda82e65c912f0c0f6dc6aa51256` 获取五个缓存脚本，逐文件校验 SHA-256。产品依赖的 Engine rev 仍由 Cargo.toml/Cargo.lock 管理；自动化 pin 独立维护，避免升级业务依赖时偷偷改变缓存实现。`.github/actions/engine-build-cache` 只负责共同的 CI 装配和无凭据 CAS 存储，没有复制另一套 MBX/R2 策略。

本地 `bun mbx:exec -- <command>` 也使用该入口。只在 Cargo 子进程移除冲突的 sccache wrapper，关闭增量编译、target views 和 target seed，不修改全局 Cargo 配置或共享 target。

Windows 继续原有 sccache/R2 路径，和 MBX 不叠用。上游 1.18.0 已提供 Windows x64/arm64 包，但 Engine 固定入口没有 Windows 支持；需要单独完成原生入口与主机验证，不能用 Unix 包装脚本假装覆盖 Windows。

## 后端与权限

复用私有桶 `uniclipboard-build-cache`，MBX namespace 为 `engine/mbx`，对象位于 `engine/mbx/v1/`。Windows 继续 `desktop/sccache/v1/windows-*`，旧对象不删除，也不能转换成 MBX 命中。

使用现有 `desktop-build-cache-reader` / `desktop-build-cache-writer` environments 内的三个 secret 名称：

- `BUILD_CACHE_R2_ENDPOINT`
- `BUILD_CACHE_R2_ACCESS_KEY_ID`
- `BUILD_CACHE_R2_SECRET_ACCESS_KEY`

sidecar 只在受保护 main 的 push 请求 read-write，其他事件请求 read-only；MBX 原生策略和实际桶令牌仍是最终约束。Linux 独立打包、PR、coverage 和 membership 绑定现有 reader，只请求读取；历史 file-key-store 回归继续不接触 secrets，只使用本地 MBX。secret 名称存在不证明令牌能访问 `engine/mbx/v1/`，需实际 R2 恢复验证；本改动不修改远端权限。

没有凭据时继续本地 MBX；部分凭据或非 HTTPS 端点拒绝装配。凭据只进入 runner 临时 0600 文件，GITHUB_ENV 保存路径。Linux daemon 容器只读挂载此文件和固定工具快照，写入单独统计目录；原始值不写入缓存、产物或命令参数。

## 存储边界与发布

macOS/Linux 不从 Actions 恢复 target。sidecar 无 R2 时按 ref 保存本地 CAS；Linux 缓存只含 Cargo registry/git 下载和 MBX CAS。容器仍使用自己独立的 target。构建参数、Engine rev、签名、Sentry 符号上传、安装包和发布检查不变。

共享桶不保证任意构建互相命中。MBX 按编译器身份、完整编译参数、输入内容、发现的环境输入和受支持链接器/SDK 身份分键。不同 target、features、profile、RUSTFLAGS 或源码可以合法未命中。Engine 作为 workspace member 与 Desktop 的 git dependency 还可能有源路径、Cargo metadata、依赖图或 profile 差异；不启用 `share_workspace_root` 去改变调试路径、`file!()` 或 panic 语义。

## 验证

每次 Cargo 会话 JSON 上传为 `mbx-sidecar-*` 或 `mbx-linux-package-*`。实际复用须使用两个空 target 并检查 `hits`、`restored_output_files`；R2 验证还必须使用两个独立 CAS，消费者 `downloaded_bytes > 0`、`uploaded_bytes == 0`，远端及后台错误为零，最后运行恢复出的产物。

本地 macOS 小型真实 Engine 内容哈希消费者已验证跨目录恢复和源码/feature/profile/RUSTFLAGS 分键，不能替代完整 daemon、Linux 容器或 Windows CI 验收。自动化脚本下载也计入首次调用耗时，不把安装成本排除后宣称整条发布流水线的收益。
