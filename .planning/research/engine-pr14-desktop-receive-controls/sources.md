# Sources: Engine PR #14 桌面端接收控制

## Engine

- `https://github.com/UniClipboard/Engine/pull/14`
- 合并提交：`9a403d7ed2687902fab52afb1d31c4b9ca746a71`
- 合并时间：`2026-07-31T13:55:16Z`
- PR 说明明确要求通过 `QueryMemberSyncPreferences` 和 `UpdateMemberSyncPreferences` 的局部 patch 接入。

## Desktop

- `src/api/daemon/member.ts` 已定义完整的发送与接收偏好。
- `src/store/slices/devicesSlice.ts` 已支持接收字段的局部乐观更新和权威值回写。
- `src/components/device/PeerDetailPanel.tsx` 当前只渲染发送设置。
- `Cargo.toml` 当前锁定 `v0.20.0-rc.15`。
- `scripts/architecture/check-engine-repository.mjs` 校验同一 Release 标签和提交。

## Release status

- 截至 `2026-07-31`，最新 Engine Release 为 `v0.20.0-rc.15`，提交 `781c568106a735e54e277994fb96b4613391e2f2`。
- PR #14 在该 Release 之后合并；当前没有包含该合并提交的新 Release。
