# 移动核心发布说明

共享 P2P 核心与可选 LAN 兼容实现已经迁入独立的 `UniClipboard/core` 仓库。

desktop 仓不再构建、发布或保存移动核心源码和绑定产物。核心版本、三端绑定、LAN 兼容版本、校验清单和发布流程统一由核心仓拥有；各移动产品仓只消费固定 Release 及其校验信息。

desktop 只固定 `Cargo.toml` 中的不可变核心提交。升级核心时必须同时更新锁文件，并通过 `bun run check:core-repository` 证明不存在本地旧副本、浮动版本或自动 LAN 回退。

迁移顺序、回退规则和历史状态继续记录在 `plans/005-extract-single-core-repository.md`。
