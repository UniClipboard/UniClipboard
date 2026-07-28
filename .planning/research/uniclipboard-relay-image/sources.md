# Sources: UniClipboard relay image deployment

## UniClipboard relay repository

- `https://github.com/UniClipboard/relay`：镜像入口、环境变量、令牌约束、反向代理要求和安全边界。
- `https://github.com/UniClipboard/relay/blob/main/Dockerfile`：非特权用户、`3340` 端口、`/healthz` 和 AMD64/ARM64 运行镜像基础。
- `https://github.com/UniClipboard/relay/blob/main/.github/workflows/publish-container.yml`：`ghcr.io/uniclipboard/relay` 的标签与多架构发布流程。
- GitHub Actions run `30328030728`：当前 `latest` 的 AMD64、ARM64 构建和清单发布均成功。

## Desktop repository

- `Cargo.lock`：桌面端当前解析到 `iroh-relay 1.0.0-rc.1`。
- `docs-site/content/docs/en/guides/self-host-relay.mdx`：旧指南仍推荐 0.98 上游镜像和手工配置。
- `docs-site/content/docs/zh/guides/self-host-relay.mdx`：中文镜像页与英文页存在同样的过时部署模型。

## Live verification

- `ghcr.io/uniclipboard/relay:latest` 可公开拉取，清单包含 `linux/amd64` 和 `linux/arm64`。
- 本地以测试令牌启动镜像后，容器健康检查通过，`/healthz` 返回 `status=ok` 和 `version=1.0.0-rc.1`。
