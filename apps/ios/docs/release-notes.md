# Release Notes — end-user "What's New"

End-user-facing release notes, newest build first. This is the copy that goes
into the TestFlight **新增内容 / What to Test** field and, later, the App Store
**What's New** field for each build.

Keep it user-facing: describe what changed for the person using the app, not how
it was implemented. Engineering detail belongs in the commit messages and
`SYNC_PROTOCOL.md`; submission metadata lives in `beta-submission.md`. Chinese
is primary (the app's source language is zh-Hans); English mirrors it.

---

## 1.0 (Build 10)

### 简体中文

```
新功能
• 一台服务器,多个地址:现在可以为同一台服务器填写多个地址(局域网、Tailscale、公网)。
  App 会自动探测哪个地址当前可达并优先使用;切换网络、回到前台或同步失败时都会自动重新
  选择,无需再手动切换服务器。原先「按 Wi-Fi / 蜂窝 / Tailscale 触发切换服务器」的设置
  由此取代。
• 服务器编辑页新增地址列表管理:每个地址会自动识别类型(局域网 / Tailscale / 公网),
  「测试连接」会逐个探测所有地址,并标出「将使用」的那一个。
• 扫码配对升级:配对二维码现在携带服务器的全部地址,扫一次即可整套导入。

优化与修复
• 首页剪贴板卡片改为标准正方形,在不同尺寸设备与横竖屏下排列更整齐。
• 修复顶栏「选择 / 全选 / 完成」按钮在空间不足时文字竖排折行的问题。
• 「信任不安全证书」设置的说明更准确:该开关仅影响自签名 HTTPS 证书,普通 HTTP 连接
  不受影响。
```

### English

```
New
• One server, multiple addresses: a server can now have several addresses
  (LAN, Tailscale, public). The app probes which one is reachable and uses the
  best automatically — re-checking when the network changes, the app returns to
  the foreground, or a sync fails. This replaces the previous per-server
  Wi-Fi / cellular / Tailscale switching rules.
• New address-list editor: each address is auto-classified (LAN / Tailscale /
  public), and "Test Connection" probes them all and marks the one that will
  be used.
• Smarter QR pairing: the pairing QR code now carries all of a server's
  addresses, so one scan imports the whole set.

Improvements & Fixes
• Home clipboard cards are now true squares, lining up neatly across device
  sizes and orientations.
• Fixed top-bar buttons (Select / Select All / Done) wrapping vertically when
  space ran short.
• Clearer wording for "Trust insecure certificate": it only affects self-signed
  HTTPS; plain HTTP connections are unaffected.
```

## 1.0 (Build 7)

### 简体中文

```
新功能
• 告别频繁的「允许粘贴」弹窗:从其他 App 复制内容后回到 UniClipboard,首页会出现
  「本机有新内容」提示,点一下「粘贴」即可推送到服务器。拉取服务器上的内容依旧全自动、
  无打扰。如果你更喜欢全自动推送,可在「设置 → 自动推送本机变化」中开启(开启后系统
  仍会按需弹出「允许粘贴」)。

优化改进
• 服务器切换更直观:合并了「默认服务器」与首页「固定服务器」,现在统一为一个「当前服务器」
  —— 在任何位置选中某台服务器,都会长期生效。
• Wi-Fi 自动切换改为友好建议:连上匹配的 Wi-Fi 时会提示「是否切换到该服务器」,由你决定,
  不再悄悄改动当前服务器。
• 首页同步提示重新设计:「服务器有新内容 → 应用」与「本机有新内容 → 粘贴」统一为成对、
  对称的单行样式,更清爽。
• 新增对 Tailscale 网络的支持:可通过 Tailscale 地址(100.64.x.x 网段)连接你的自建服务器。

问题修复
• 修复点「粘贴」推送纯文本或链接时,偶尔推送空内容的问题。
• 修复剪贴板预览中,换行折叠的长链接无法选中末尾的问题。
• 修复英文系统下,系统共享扩展界面误显示中文的问题。
```

### English

```
New
• No more "Allow Paste" pop-ups: after you copy something in another app and
  come back to UniClipboard, the Home screen shows a "New on this device" hint —
  tap "Paste" to push it to your server. Pulling content from the server stays
  fully automatic and silent. Prefer the old fully-automatic push? Turn it back
  on in Settings → Auto-push device changes (iOS will then show the system
  "Allow Paste" prompt as needed).

Improvements
• Simpler server switching: the server-list "default" and the Home "pin" are now
  one single "current server" — pick a server anywhere and it stays current.
• Wi-Fi auto-switch is now a friendly suggestion: when you join a matching Wi-Fi,
  the app asks whether to switch to that server instead of silently rerouting.
• Redesigned Home sync hints: "Server has new content → Apply" and "New on this
  device → Paste" are now a matched, symmetric single-line pair.
• Tailscale support: connect to your self-hosted server over a Tailscale address
  (the 100.64.x.x range).

Fixes
• Fixed pushing empty content when tapping "Paste" for plain text or links.
• Fixed not being able to select the tail of a wrapped long link in the preview.
• Fixed the share extension showing Chinese on English-language devices.
```
