# 历史列表的有限内存窗口

历史列表与网格使用同一个记录窗口。窗口最多保留 50 条已解密记录，包含可见记录和预加载记录。窗口之外不保留条目内容、详情预览或列表图片 URL。持久化历史不受此限制。

滚动期间，窗口从可见范围上方最多 10 条开始，最多加载 50 条。停止滚动 750 毫秒后，窗口收缩到可见范围及上下各最多 10 条。历史开头和结尾按实际记录数收缩。如果可见记录多于 30 条，先保留可见记录，再按 50 条总上限分配缓冲区。

`useHistoryWindow` 按 `offset` 和 `limit` 请求当前窗口。切换窗口时先移除离开范围的记录，保留重叠部分，再请求缺少的部分。被移除的记录在反向滚动时重新读取。新请求会取消旧请求。若被取消的是刷新请求，新请求必须继承刷新要求，完整读取新范围，不得复用旧记录与新偏移混合拼接。筛选改变时从第一条重新加载。实时新增、界面删除和连接恢复会重新读取当前窗口。新增通知直接使当前窗口失效，不再另读历史头部的 50 条内容。

有限窗口最多渲染 50 条记录，因此使用原生滚动容器，不再对这个窗口做第二次虚拟化。其他未使用有限窗口的组件继续使用原有虚拟列表。窗口内容变化后，在布局阶段读取同一可见记录的位置，并补偿滚动偏移。只有记录标识序列变化才恢复位置，普通重新渲染不得覆盖用户滚动。原生滚动锚定在这个容器内关闭，避免两次补偿。

如果整个窗口都能放入视口，滚轮不会产生原生滚动事件。此时滚轮的垂直输入和 PageUp/PageDown 直接请求较新或较早的 10 条记录。窗口保留重叠记录，释放离开范围的记录，且仍最多保留 50 条。到达历史首尾后停止移动。这个输入路径由 macOS 和 Linux 共用，不通过持续自动翻页跳过历史。

图片预加载只遍历当前记录窗口。图片 URL 区分可见组件引用和缓冲引用。可见组件引用固定 URL；缓冲引用允许缓存淘汰。达到 50 个 URL 时，只能淘汰未被可见组件使用的 URL。若全部 URL 都被固定，新请求按繁忙状态退避重试，不得撤销可见 URL，也不得临时超出上限。最后一个使用者离开时撤销 URL，并取消未完成的请求。旧请求不得重新填充已释放的缓存。图片缓存另有 50 个 URL 的硬上限。锁定状态继续由 Engine 和宿主授权控制，缩略图不可用时不读取原图兜底。繁忙条目进入下一轮重试，不得阻塞其他条目。可见请求和预加载共用带随机抖动的指数退避，延迟上限为 8 秒加抖动。

## 端到端验收

先构建真实 daemon、Go CLI 和 macOS acceptance 应用包，再运行：

```bash
python3 apps/gui-go/e2e/history_window_run.py \
  --repo . --app target/acceptance/UniClipboard\ E2E\ Test\ Build.app \
  --out .planning/history-window-list.json
python3 apps/gui-go/e2e/history_window_run.py --grid \
  --repo . --app target/acceptance/UniClipboard\ E2E\ Test\ Build.app \
  --out .planning/history-window-grid.json
```

场景使用独立 HOME、独立 profile 和 120 张合成图片。系统剪贴板保持禁用。场景检查双向滚动顺序、记录与图片缓存上限、空闲收缩、窗口外图片 URL，以及历史头部和深层分页位置的新增与删除。新增图片经过真实接收路径。删除经过界面确认流程。场景必须先确认界面总数从 120 变为 121，再变回 120。新增和删除前后的同一可见条目垂直偏移不得超过 2 像素。保留的可见记录必须保持同一个 DOM 节点，图片必须已成功解码。结果记录源码、场景和二进制哈希。Windows 需要在相应原生桌面上另行验收。


大视口场景使用真实原生 WebView，并通过窗口 API 调整大小。场景必须确认 50 条全部可见，且内容高度不超过视口高度，再验证前后双向导航。脚本化滚轮事件不等于物理触控板手感验收。

```bash
python3 apps/gui-go/e2e/history_window_run.py --grid --large-viewport \
  --repo . --app target/acceptance/UniClipboard\ E2E\ Test\ Build.app \
  --out .planning/history-window-large.json
```

Linux 构建必须使用 `UC_UI_PLATFORM=linux` 和 `gtk4,e2e`。在独立 D-Bus 会话及 Xvfb 中运行同一个场景：

```bash
dbus-run-session -- xvfb-run -a -s '-screen 0 2560x1920x24' \
  python3 apps/gui-go/e2e/history_window_run.py --repo . \
  --gui target/gui-go/gui-go --grid --large-viewport \
  --out .planning/history-window-linux-large.json
```

`.github/workflows/history-window-gui-e2e.yml` 提供独立 Linux 运行器。该场景验证 WebKitGTK 的视口、导航和缓存上限，不验证 Wayland、物理输入或完整 Linux 桌面交互。
