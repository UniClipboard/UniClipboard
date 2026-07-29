# Brief: Engine v0.20.0-rc.11 desktop 集成

**Date:** 2026-07-30
**Status:** Locked
**Implemented:** 2026-07-30
**Branch:** main
**Research question:** desktop 应如何采用 `UniClipboard/Engine` 的 `v0.20.0-rc.11`，并保证来源与版本锁定一致？

## Recommendation

将 desktop 的共享引擎依赖从重命名前的仓库地址和 `core-v*` 标签切换到 `UniClipboard/Engine` 的 `v0.20.0-rc.11`，并同步更新锁文件、来源校验入口和当前维护文档。

## Key findings

1. desktop 当前标签名虽然也是 `rc.11`，实际锁定的是旧标签提交 `b742208f230b779cc4bc741e5b190cb7134d18db`。
2. 新 Release 标签指向 `8f9d09789cbe14d3d6bd328edca17fa6a0b14ef9`，比旧标签多 7 个提交。
3. Engine 已将公开仓库身份、Release 标签和发布资产命名统一到 `UniClipboard/Engine` 与 `v*`。

## Approach

- 使用新仓库的不可变 Release 标签。
- 让 `Cargo.toml`、`Cargo.lock` 和来源校验脚本锁定同一个提交。
- 将仍在使用的检查命令与维护文档统一到 Engine 名称。
- 保留历史计划中的旧名称，不改写历史事实。

## Constraints

- desktop 只能通过公开的 `uc-engine` 和 `uc-observability-contract` 接入。
- LAN 兼容能力仍必须显式启用，不能成为自动回退路径。
- 不在 desktop 恢复已迁出的 Engine 内部包。

## Implementation checklist

- [x] 更新依赖仓库、标签和锁文件
- [x] 更新来源校验脚本及调用入口
- [x] 更新当前维护文档
- [x] 通过来源校验、编译和相关测试

## Alternative considered

继续使用旧仓库地址并仅改标签不可取：旧地址依赖 GitHub 跳转，而且新的 `v*` 标签已经是正式发布契约。
