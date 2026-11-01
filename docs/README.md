# UniClipboard 桌面端文档

桌面端由 Go/Wails 宿主、React 界面、独立 daemon 和 Go CLI 组成。daemon 通过固定提交的 `uc-engine` 使用核心业务能力。Engine 的内部实现由独立仓库 `UniClipboard/Engine` 维护。

## 开发入口

- [项目概览](overview.md)：运行时结构、源码位置和开发命令。
- [产品方向与安全底线](../VISION.md)：产品范围、密文持久化和锁定决策。
- [仓库规则](../AGENTS.md)：按任务读取对应规则。
- [桌面宿主](../apps/gui-go/README.md)：Go/Wails 开发、构建与打包。
- [Go CLI](../apps/cli-go/AGENTS.md)：用户命令行实现。

## 当前架构

- [Engine 消费边界](architecture/engine-repository-checks.md)：固定版本、公开入口和迁出目录检查。
- [桌面启动装配](architecture/bootstrap.md)：宿主能力准备与 Engine 启动的责任边界。
- [跨平台核心接口](architecture/uc-engine-interface.md)：操作、事件和生命周期。
- [核心诊断与桌面日志](architecture/engine-diagnostic-runtime.md)：诊断合同与日志文件所有权。
- [宿主命令](architecture/gui-go-host-commands.md)：从 Go 合同生成的命令表。
- [启动与资料升级](architecture/startup-upgrade-progress.md)：启动进度和升级状态。
- [自动连接](specs/automatic-peer-connections.md)：宿主与 Engine 的责任边界。
- [接收结果与选择性重试](architecture/receive-item-results.md)：逐文件结果、重试和取消语义。

## 验证与发布

- [GUI E2E 编写规范](testing/gui-e2e-authoring.md)：新增测试的分类、注册、隔离、运行与工件要求。
- [明文探针](development/plaintext-probe.md)：持久化数据和日志的内容泄漏检查。
- [编译缓存](ci/shared-compiler-cache.md)：本地与 CI 缓存配置。
- [发布流程](release-workflow.md)：构建、签名、发布和更新渠道。
- [无头节点部署](../deploy/vps/README.md)：Docker 与 Caddy 部署。
- [pacman 软件源](architecture/pacman-repository.md)：签名快照与上线验收。

## 历史资料

[Tauri 宿主退役记录](architecture/gui-go-tauri-retirement.md) 保留迁移范围与当时的验证结果。标记为历史文档的指南不再作为开发入口。ADR 保留决策背景；实现位置与当前行为以当前源码和对应现状文档为准。
