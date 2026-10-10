# GitCode Release Mirror Action 集成冒烟验收计划

本文件在实现测试 workflow **之前**提交，先写失败场景与端到端验收标准，再写实现。

## 目的

验证已发布的第三方 Action `mkdir700/gitcode-release-mirror`（固定完整提交 SHA）能否在
UniClipboard 的真实 GitHub runner 上，经 SSH（严格 host key 验证）让上海中转 host 把合成小文件
上传到 GitCode 独立测试 release，并通过匿名回读校验 SHA-256 与大小。

本冒烟 **不** 替换、不修改、不触发现有生产镜像流程（`mirror-desktop-gitcode.yml`、`release.yml`），
不写 FlareRelease，不创建 `v*` tag，不触碰任何已有正式 release 资产。

## 固定输入

| 项 | 值 | 核对方式 |
| --- | --- | --- |
| Action 仓库 | `mkdir700/gitcode-release-mirror`（公开） | `gh repo view` |
| Action 版本 | `v1.0.0`（注解 tag `baa146b1…` → 提交 `5055ef17b7302e27ca53cbe5f6e44722f7e55f6b`） | `git rev-parse v1.0.0^{commit}`，且等于 `main` HEAD |
| workflow 引用方式 | `uses: mkdir700/gitcode-release-mirror@5055ef17b7302e27ca53cbe5f6e44722f7e55f6b` | 完整 SHA，不用 tag |
| 若仓库将转移组织 | 以实际来源为准，重新核对 owner 与 SHA 后再改 `uses:` | 本文件需同步更新 |

## 拓扑与凭据边界

- GitHub runner（`ubuntu-latest`）→ Action（Node 24）→ 系统 OpenSSH（`StrictHostKeyChecking=yes`，仅使用提供的 known_hosts）
  → 上海 host 专用账号的 forced command（新版本目录，独立白名单配置）→ 从
  `raw.githubusercontent.com` 的固定提交路径下载合成文件 → GitCode API 创建独立测试 release 并上传。
- 复用（仅按名称确认，不读取值）：组织 secret `GITCODE_RELEASE_TOKEN`、仓库变量
  `MIRROR_SSH_HOST`、`MIRROR_SSH_KNOWN_HOSTS`、`GITCODE_OWNER`、`GITCODE_REPO`。
- 新建且仅用于冒烟：专用 SSH key、独立 GitHub environment（分支策略只放行冒烟分支）。
  现有 `mirror` environment（仅 `main` 可用）、`MIRROR_SSH_KEY`、两个现有 forced command 不改动。
- 触发方式只有 `workflow_dispatch`；`permissions: contents: read`；不使用 `pull_request_target`。

## 测试资源命名

- GitCode 测试 tag / release：`test-action-smoke-<UTC 日期>-<序号>`，`prerelease: true`。
  tag 不允许 `/`（Action 的 tag 校验正则不含斜杠），因此用连字符前缀。
- 合成资产放在固定提交的 `.github/gitcode-action-smoke/` 下：`original/`、`conflicting/`
  两个目录里的同名文件字节不同，`direct/` 里是 direct 对照文件。均为非敏感合成文本。

## 失败场景（先写，必须观察到）

| # | 场景 | 期望 | 反证（说明没通过） |
| --- | --- | --- | --- |
| F1 | known_hosts 换成无关的 host key | Action 失败退出码非 0；host 命令未被启动；GitCode 无该 tag 的 release | 连接成功，或出现 release |
| F2 | `sources` 中 sha256 故意写错 | 该文件 `failed`，阶段为校验；GitCode 无该资产 | 资产出现在 release 中 |
| F3 | 同名异字节（`conflicting/` 对已存在的 `original/` 资产） | 文件 `failed`，`conflict`；`failed-count=1`；步骤退出码非 0 | 原资产被覆盖或删除 |
| F4 | F3 之后独立匿名回读原资产 | size 与 SHA-256 等于 `original/`；资产 id 与创建时间不变 | 任何一项变化 |
| F5 | 日志与 receipt 泄漏检查 | 运行日志、receipt、artifact 中不含 `access_token=`、私钥头、token 值 | 命中任一模式 |
| F6 | 来源不在 host 白名单（越权 URL） | host 拒绝，退出码为拒绝类；不下载 | 下载发生 |

## 成功场景（必须观察到）

| # | 场景 | 期望 |
| --- | --- | --- |
| P1 | SSH 首次镜像 `original/` | receipt `mirrored`；GitCode 创建 release 与资产 |
| P2 | 匿名回读（Action 之外，由 runner 的 curl 独立执行） | size、SHA-256 与源文件一致 |
| P3 | 同一 tag 重复运行 | receipt `reused`；资产 id 不变；host 没有新增上传 |
| P4 | direct 对照（runner 本地小文件，另一文件名） | `mirrored`，仅作对照，不代表真实安装包性能 |
| P5 | receipt artifact | 上传 receipt 与 SHA-256 索引；索引可用 `shasum -a 256 -c` 复核 |

## 证据要求

receipt/README 入口须包含：Action 版本与完整 SHA、冒烟分支与 source SHA、workflow 与 run URL、
runner 架构、host 环境（OS、OpenSSH、Node）与 core SHA、测试 tag / 文件名 / hash、每步结果与限制、
工件 SHA-256 索引。并区分：本地/mock 证据（t-0228 的 38 个场景）与本次真实 GitHub runner 与真实 GitCode 的证明。

## 不在本次范围

- 真实安装包大小（20–120 MB）下的性能；本次只用小文件。
- FlareRelease 登记、R2、生产 `release.yml` 迁移。
- 删除或清理 GitCode 测试资产（需另行授权）。
- 发布、上架 Action 新版本。

## 已知限制

- `workflow_dispatch` 只能触发已存在于默认分支的 workflow 文件。已在本分支实测：
  `gh workflow run gitcode-release-mirror-smoke.yml --ref ci/gitcode-release-mirror-action-smoke`
  返回 `HTTP 404: workflow … not found on the default branch`。因此 smoke workflow 必须先合并进 `main`
  才能被 dispatch；合并只增加一个仅 `workflow_dispatch` 触发的文件，不改动任何生产流程。
- 现有 `mirror` environment 仅允许 `main`，其 SSH key 绑定旧 Python wrapper，无法测试新 Action；
  因此另建 `gitcode-mirror-smoke` environment 与专用 key，不复用也不修改旧的。
- GitCode API 使用 `access_token` 查询参数，token 可能出现在 GitCode 服务端日志中，此点不受我们控制。

## 解除阻塞后的执行步骤（每步均需维护者确认）

1. 审阅并合并本 PR（仅新增 smoke workflow、合成资产、host 安装脚本、本文档）。
2. 本地生成专用 key：`ssh-keygen -t ed25519 -N '' -f smoke_key`；创建 environment
   `gitcode-mirror-smoke`（部署分支仅限 `main`），用 `gh secret set GITCODE_ACTION_SMOKE_SSH_KEY --env gitcode-mirror-smoke < smoke_key`
   写入私钥后删除本地私钥文件；设置变量 `GITCODE_ACTION_SMOKE_SSH_USER=gitcode-smoke`。
3. 在 host 上以 root 运行 `scripts/remote/install-gitcode-release-mirror-smoke-host.sh`，
   参数为固定提交的 Action checkout、公钥文件、将要 dispatch 的 `main` 提交完整 SHA。
   安装前后记录 `/opt/uniclip-mirror/*` 与 `/home/uniclip-mirror/.ssh/authorized_keys` 的 SHA-256，应保持不变。
4. `gh workflow run gitcode-release-mirror-smoke.yml --ref main -f tag=test-action-smoke-<YYYYMMDD>-1`，
   再用同一 tag 运行一次观察重复复用。
5. 用 `gh run view --log` 对完整日志搜索 `access_token=`、`PRIVATE KEY`，并保存 receipt artifact。

## 本地已验证（不等于真实 runner / GitCode 证明）

- `actionlint` 与 `shellcheck` 通过；host 配置模板渲染后是合法 JSON，文件名正则只放行 `gitcode-action-smoke*.txt`。
- 固定提交 `5055ef17…` 上 `npm ci && npm run check-dist` 通过（发布的 `dist/` 由该源码构建）。
- 只读核对 host：CentOS 7、OpenSSH 7.4p1（支持 `restrict`）、已有 Node v22.22.1 可独立运行；
  `sshd_config` 无 `AllowUsers`/`AllowGroups`；SELinux 为 Disabled。
