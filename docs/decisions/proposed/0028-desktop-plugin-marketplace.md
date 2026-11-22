# 0028 · 桌面端在线插件市场与简化插件管理

- 状态：已实现。`UniClipboard/unic-plugins` 静态 `registry.json` 与 `unic-plugin-wenshushu` `v0.1.1` 六平台预编译 Release 已发布，真实远端目录拉取、无源码构建预编译安装（`Build=false`）、`unicd` 重启握手与 BoardUI 渲染均已通过端到端验证；待用户视觉确认与原生窗口交互验收。
- 日期：2026-11-22。
- 关联：[0004 剪贴板流水线与插件](0004-clipboard-pipeline-lua.md)、[0010 插件设置 Schema](0010-plugin-settings-schema.md)、[0013 BoardUI](0013-mygo-uikit.md)、[0027 文叔叔临时分享](0027-wenshushu-share-action.md)。
- 落地目标：[插件契约](../../contracts/plugin-contract.md)、[本地服务契约](../../contracts/local-service-contract.md)、[桌面运行时架构](../../architecture/desktop-runtime.md)。
- 交互原型：[prototype/plugins.html](../../../prototype/plugins.html)。

## 背景

既有插件系统已完成 Go 独立进程 RPC、Lua 沙箱、Schema 设置表单和 GitHub 仓库安装器（`unic plugin install/uninstall/rollback`），但桌面端「设置 → 插件」存在明显的理解与操作门槛：

1. 安装、卸载、回滚与执行顺序仍停留在命令行，桌面用户必须手动复制 GitHub URL、Ref、子目录与 `--build` 参数。
2. 外部插件列表只展示裸 ID 与版本号，缺少中文名称、简介、状态与快捷开关；非同步外部插件（如 `unic.wenshushu`）在详情页中会错误显示“未运行”。
3. 零配置插件（如文叔叔临时分享）仍暴露出空洞的配置详情骨架，普通用户难以快速理解“怎么用”与隐私边界。

## 决策

### 1. 采用 Obsidian 风格的在线插件市场作为主入口（桌面端）

- **主界面极简**：桌面端「设置 → 插件」以「插件市场」卡片作为首要入口，下方分为「已安装插件」与「内置功能」两块清晰列表。
- **按用户目的而非技术能力组织**：市场提供搜索与场景分类（全部 / 官方推荐 / 分享与导出 / 文本效率 / 智能整理 / 同步扩展），点击插件即在侧边/详情展示简介、使用步骤、隐私边界与一键操作（安装 → 启用 / 选项 / 更新 / 卸载）。
- **零配置插件不展示无意义的设置入口**：当 `HasSettings == false` 且非同步后端（例如 `unic.wenshushu`）时，列表和详情不展示空的设置表单，聚焦“开关、使用说明与卸载”。
- **开发与高级能力收纳到次级入口**：执行顺序（`plugins.order`）、全局错误处理策略（`plugins.on_error`）、来源信息/回滚与“从链接手动安装（开发者）”统一收纳在右上角更多菜单与详情折叠区，避免干扰常规用户。

### 2. 静态 JSON 市场清单（Registry）+ 本地缓存 + 内置官方兜底

- 市场数据源采用静态 `registry.json`（版本 `version = 1`），默认地址为 `https://raw.githubusercontent.com/UniClipboard/unic-plugins/main/registry.json`，支持通过环境变量 `UNIC_PLUGIN_REGISTRY_URL` 覆盖（`off` 表示禁用远端拉取，仅使用内置目录，便于离线与确定性测试）。
- 服务端将校验通过的远端清单缓存到 `$UNIC_HOME/plugins/marketplace-cache.json`（大小上限 512 KiB，默认 TTL 15 分钟，支持显式刷新）。
- 远端不可达、未发布或格式无效时，服务自动回退到本地有效缓存或客户端内置官方清单（首批包含 `unic.wenshushu`），保证桌面端在离线或无网络环境下仍可浏览并给出明确来源状态（`remote` / `cache` / `bundled`）。
- **安装底层完全复用现有安全边界**：从市场一键安装底层仍调用 `luascripts.Install`，遵循既有不可变 Commit 锁定、清单校验、预编译优先 + 可选 `--build` 兜底、SHA-256 完整性校验、`installed.lock` 原子写入以及“首次安装默认停用、重启后生效”规则，不引入第二套安装或执行信任模型。

### 3. 本地服务新增插件生命周期与市场 API

在既有 `/v1/plugins` 与 `/v1/plugins/{id}/settings` 基础上补充：

- `GET /v1/plugins/marketplace?refresh=1`：返回校验后的市场目录、数据来源与可选提示。
- `POST /v1/plugins/install`：按仓库/Ref/Path/Build/Replace/Enable 参数执行安装，可选写入启用列表，返回最新 `PluginInfo`。
- `DELETE /v1/plugins/{id}`：停用并从 `installed.lock` 移除，重启后清理旧文件，保留历史与配置。
- `POST /v1/plugins/{id}/rollback`：切换回不可变保留的上一版本，重启后生效。
- 扩展 `PluginInfo`：补充动作清单（`actions`）、订阅事件（`events`）、来源仓库/Commit 以及是否存在可回滚上一版本（`has_previous` / `previous_version`），同时修正桌面端非同步外部插件运行状态判断（结合 `RunningVersion` 与 `Configured` / `Restart`）。

## 范围边界

- 本次仅覆盖桌面端（`unic-desktop` + `unicd` 本地服务）；移动端（Expo）暂不在本次范围内。
- 文叔叔插件（`unic.wenshushu`）保持零配置设计，不新增插件专属 Schema。
- 下载量统计、用户评分与独立市场后端服务不在第一版范围内；静态清单满足离线缓存与极简运维需求。
