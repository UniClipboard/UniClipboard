# Brief: 使用 UniClipboard relay 镜像简化自建中继

**Date:** 2026-07-28
**Status:** Locked
**Implemented:** 2026-07-28
**Research question:** self-host-relay 指南应如何改用 UniClipboard 官方镜像，并减少用户需要理解和维护的部署细节？

## Recommendation

把 `ghcr.io/uniclipboard/relay:latest` 作为指南唯一推荐的 relay 实现，并用 Docker Compose 同时运行 relay 与 Caddy。relay 只在容器网络监听 HTTP，Caddy 负责公网 TLS；用户只需准备域名、Docker 和一个访问令牌。

## Key findings

1. 官方镜像公开可拉取，同时提供 Linux AMD64 和 ARM64 版本；当前 `latest` 的发布流程已成功。
2. 镜像和桌面端都使用 `iroh-relay 1.0.0-rc.1`，不存在旧指南中 `0.98.x` 镜像的版本错配问题。
3. 镜像默认以非特权用户运行，监听 `3340`，自带 `/healthz` 健康检查，并强制配置一个访问令牌。
4. 镜像不负责公网 TLS；官方 relay README 要求生产环境使用支持 WebSocket 的反向代理，并保留 `Authorization` 请求头。
5. 旧指南推荐的 `n0computer/iroh-relay:0.98.2-docker2`、`config.toml`、源码构建、systemd、NodeId allowlist 和 UDP/7842 已不再对应当前 UniClipboard relay 的部署模型。

## Approach

### What to use

- `ghcr.io/uniclipboard/relay:latest`：由 UniClipboard 维护并与桌面端协议版本对齐。
- Docker Compose：在一个文件中声明 relay、Caddy、证书卷和重启策略。
- Caddy：自动申请和续期公网证书，并代理 WebSocket。
- `UC_RELAY_TOKEN`：用 `openssl rand -hex 32` 生成，保存在权限为 `0600` 的 `.env` 中。

### What NOT to use

- 上游 0.98 Docker 镜像：与当前客户端协议版本不一致。
- 源码构建 + systemd 作为主流程：增加编译、证书和服务管理负担。
- NodeId allowlist：官方镜像的访问控制来源是单个访问令牌。
- 直接把容器的 `3340` 暴露到公网：生产环境必须通过 HTTPS 入口。

## Constraints

- 文档必须同时更新英文和中文版本，标题层级和操作步骤保持对应。
- 不得在示例、日志或仓库文件中写入真实访问令牌。
- Caddy 配置默认不启用访问日志，避免浏览器客户端的 `token` 查询参数进入日志。
- 已有 nginx、Caddy 或其他反向代理的用户可以复用现有入口，但必须保留 `Authorization` 并支持 WebSocket。
- `latest` 只有在执行 `docker compose pull` 后才会更新；需要固定版本的用户应改用包页面提供的版本标签或镜像摘要。

## Implementation checklist

- [x] 用官方镜像和 Compose 重写中英文部署主流程
- [x] 删除旧 0.98 镜像、源码构建、手工 TLS 和 allowlist 说明
- [x] 补充令牌保存、健康检查、更新和现有反向代理说明
- [x] 更新客户端接入、日志命令、代理直连和排障内容
- [x] 运行双语一致性检查、格式检查、类型检查和正式构建

## Open questions

- 官方 relay 仓库当前还没有稳定版本发布；指南先使用 `latest`，并说明固定版本的方法。
