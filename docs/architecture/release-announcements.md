# 桌面版本通知

主窗口侧边栏的管理区域按顺序显示「通知」「管理设备」「设置」。通知入口显示通知数量，点击后打开通知 Sheet。主窗口不显示通知横幅。展开与折叠状态共用同一份管理操作定义。Windows 复用既有紧凑侧边栏；macOS、Linux 在原本完全隐藏侧边栏的状态下显示管理操作栏。

Sheet 显示通知标题、正文、等级和发布时间。通知卡片不提供「已读」操作。客户端不保存本机阅读状态，不自动把打开或关闭 Sheet 解释为已读。通知由服务端的发布、撤回和到期规则决定是否显示。

设置中的通知入口继续复用同一列表、请求缓存。

Sheet 的「刷新」位于底部左侧，「完成」位于底部右侧。两者使用平台组件库的正常按钮尺寸。正文通过统一内边距避开容器的圆角。设置页没有 Sheet 页脚，因此保留列表上方的刷新入口。

## 请求路径与职责

`useHistoryManagement` / `NotificationsSection` → RTK Query → `api/announcements.ts` → 生成的 `HostService.GetReleaseAnnouncements` → 界面进程私有管道 → 常驻 Go 宿主 → FlareRelease `/announcements`。

宿主发送 `product=desktop`、`target_version=<当前安装版本>`、从安装版本识别的 `channel` 和界面语言 `locale`。通知渠道描述安装版本，不使用用户选择的下一次更新渠道。FlareRelease 当前仅支持 Desktop 的 `stable` 与 `alpha`；其他安装渠道省略 `channel`，只获取产品级和版本级通知。

FlareRelease 负责发布状态、开始时间、结束时间、撤回版本和目标范围筛选。客户端保留服务端的等级优先、发布时间倒序排列。未知或撤回的版本返回空列表。公开接口的跨域白名单仅包含官网，因此请求由 Go 宿主持有。

接口依据：[公开接口](https://github.com/UniClipboard/FlareRelease/blob/main/src/server/public-app.ts)、[查询规则](https://github.com/UniClipboard/FlareRelease/blob/main/src/server/db/public-store.ts)、[跨域与缓存](https://github.com/UniClipboard/FlareRelease/blob/main/workers/public-worker.ts)。

## 生命周期与数据

主窗口在前台时每五分钟检查一次通知。窗口重新获得焦点或网络恢复时也检查。关闭主窗口后不继续轮询。RTK Query 只在内存中保存响应。请求失败保留上次成功获取的通知；请求成功后以服务端列表替换缓存。

请求超时为 8 秒。响应上限为 1 MiB。宿主校验响应结构、等级和时间。请求失败不解释为空列表。不同语言使用不同缓存键，旧语言结果不会覆盖当前语言。

通知响应只在内存中保存，不写入配置、数据库或磁盘缓存。客户端不新增阅读状态持久化。

Markdown 复用更新说明渲染器，跳过 HTML，并禁用通知中的图片。链接通过宿主打开外部浏览器。

`requireAcknowledgement` 显示阅读提示。客户端不向服务端提交确认。不新增更新安装门禁或系统推送。

## 验证

从仓库根目录执行客户端 HTTP 集成验证。此命令启动临时 loopback HTTP 服务，使用合成通知，并在退出前关闭服务：

```bash
(cd apps/gui-go && go run ./cmd/announcementsprobe) > /tmp/announcements-receipt.json
```

该回执验证生产客户端、查询参数、刷新后的空列表、错误响应、取消和离线情况。它不证明真实 WebView 或线上通知发布行为。

界面候选复用真实平台组件和共享管理操作，宿主响应为合成数据：

```bash
bun run e2e:gui layouts main-notifications --platform macos
bun run e2e:gui layouts main-notifications --platform windows
bun run e2e:gui layouts main-notifications --platform linux
bun run e2e:gui layouts release-notifications --platform macos
```

`main-notifications` 覆盖明暗主题、管理操作顺序、展开与折叠、点击后打开 Sheet、无已读按钮、关闭后重新打开仍可阅读、刷新可获取新通知。`release-notifications` 覆盖设置入口、外部链接、请求失败和恢复。

这些候选尚未加入默认矩阵。截图属于隔离浏览器夹具，不代表完整历史页面、真实原生窗口、跨窗口存储或线上通知发布验收。
