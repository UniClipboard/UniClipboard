# 发布渠道凭据说明

本文记录各包管理器发布渠道所需的 GitHub Actions secret、最小权限要求和轮换策略。

## 概览

| Secret | 渠道 | 当前状态 | 负责人 |
|---|---|---|---|
| `HOMEBREW_TAP_TOKEN` | Homebrew Tap（CLI） | ✅ 已配置 | 仓库 owner |
| `WINGET_TOKEN` | winget（Windows 官方包管理器） | ❌ 缺失 | 仓库 owner |
| `HOMEBREW_CASK_TOKEN` | Homebrew 官方 Cask（macOS GUI） | ❌ 缺失 | 仓库 owner |
| `CHOCOLATEY_API_KEY` | Chocolatey（Windows 社区仓库） | ❌ 缺失 | 仓库 owner |

三个缺失的 secret 导致 v0.16.0 及之后 stable release 的 winget、homebrew-cask、chocolatey 发布 workflow 全部失败。

---

## WINGET_TOKEN

**用途**：`wingetcreate update --submit` 时，工具代替你 fork `microsoft/winget-pkgs`、计算 hash、生成 manifest 并向上游提 PR。token 的 GitHub 账号就是 fork 的持有者。

**Token 类型**：classic PAT（wingetcreate 不支持 fine-grained）

**最小 scope**：`public_repo`

**签发账号**：建议用专用 bot 账号（如 `uniclipboard-release-bot`），避免泄露等价于泄露个人账号。

**创建步骤**：
1. 用 bot 账号登录 GitHub。
2. Settings → Developer settings → Personal access tokens → Tokens (classic) → Generate new token。
3. 勾选 `public_repo`，过期时间 90 天。
4. 复制 token，在 `UniClipboard/UniClipboard` 仓库 Settings → Secrets → Actions 新建 `WINGET_TOKEN`。

**轮换**：过期前 7 天在日历添加提醒，换新 token 后更新 secret 即可。无需手动 fork——wingetcreate 每次自动处理。

**相关 workflow**：`.github/workflows/winget-publish.yml`

---

## HOMEBREW_CASK_TOKEN

**用途**：向 fork（默认 `UniClipboard/homebrew-cask`）推送更新的 Cask 文件，然后调用 `gh pr create` 向 `Homebrew/homebrew-cask` 上游开 PR。

**Token 类型**：fine-grained PAT（推荐）或 classic PAT

**最小权限（fine-grained）**：
- 授权仓库：`UniClipboard/homebrew-cask`（即你持有的 fork）
- Contents：Read and write
- Pull requests：Read and write（用于向上游仓库开 PR 时的 `gh` 鉴权）

**前置条件**：token 所属账号必须已 fork `Homebrew/homebrew-cask` 并命名为 `UniClipboard/homebrew-cask`（或修改 workflow 中的 `fork_repo` 默认值）。

**创建步骤**：
1. 确认 `UniClipboard/homebrew-cask` fork 存在（若不存在，用仓库 owner 账号 fork `Homebrew/homebrew-cask`）。
2. 在同一账号下生成 fine-grained PAT，按上方权限配置，90 天过期。
3. 在 `UniClipboard/UniClipboard` 仓库新建 secret `HOMEBREW_CASK_TOKEN`。

**相关 workflow**：`.github/workflows/homebrew-cask.yml`；另见 `docs/packaging/homebrew-cask.md`。

---

## CHOCOLATEY_API_KEY

**用途**：`choco push` 时向 `https://push.chocolatey.org/` 上传打好的 `.nupkg` 包。

**Token 类型**：Chocolatey.org API key（非 GitHub token）

**获取方式**：
1. 登录 [chocolatey.org](https://chocolatey.org)，使用包维护者账号。
2. 进入账号页面 → Account → API Key，复制 key。
3. 若尚未在 Chocolatey 上架 `uniclipboard` 包，首次需要人工提交审核（moderation）；通过后后续版本可自动推送。

**创建步骤**：
1. 在 `UniClipboard/UniClipboard` 仓库新建 secret `CHOCOLATEY_API_KEY`，值为上述 key。

**注意**：Chocolatey API key 不设过期，但建议每 6 个月轮换一次（在 chocolatey.org 账号页面重新生成）。

**相关 workflow**：`.github/workflows/choco-publish.yml`

---

## 安全原则

- **单一职责**：每个渠道一个专用 secret，名称即用途，出问题时可独立撤销而不影响其他渠道。
- **最小权限**：优先 fine-grained PAT，限定到对应 fork 仓库；只在工具强制要求时退回 classic PAT（仅勾 `public_repo`，不勾 `repo` 全权限）。
- **专用账号**：`WINGET_TOKEN` 和 `HOMEBREW_CASK_TOKEN` 建议由专用 bot 账号签发，避免与个人身份绑定。
- **过期策略**：GitHub PAT 设 90 天过期 + 日历提醒；Chocolatey API key 建议每 6 个月手动轮换。

## 已知问题记录

### v0.16.0 发布时三渠道失败

- **winget**：`WINGET_TOKEN` 缺失 → wingetcreate 报 `String cannot be empty (Parameter 'token')`。
- **homebrew-cask**：`HOMEBREW_CASK_TOKEN` 缺失 → `actions/checkout` 报 `Input required and not supplied: token`。
- **chocolatey**：`CHOCOLATEY_API_KEY` 缺失（未到 Push 步骤）；更早触发的 bug 是 SHA256 校验逻辑在 PowerShell 7 下，`Invoke-WebRequest .Content` 对 `application/octet-stream` 返回 `byte[]` 而非 `string`，导致 `-match` 永远失败。该 bug 已在 `.github/workflows/choco-publish.yml` 中修复（改用 `RawContentStream`）。
