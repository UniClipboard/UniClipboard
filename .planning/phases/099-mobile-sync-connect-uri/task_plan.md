# Task Plan: 099 · 移动端扫码接入协议 `uniclipboard://connect`

## 目标

为移动端注册流程引入版本化深链协议 `uniclipboard://connect`,把 `base_url / username / password / 扩展元数据`编码进 **单个二维码**,让 iOS Shortcut / Android SyncClipboard 兼容客户端 / 未来原生 App **免输入接入**,消除 `MobileSyncCredentialModal` 中"用户肉眼抄写三栏"的体验缺陷。

跟踪 issue: <https://github.com/UniClipboard/UniClipboard/issues/789>

## 当前阶段

阶段 0-4A 已提交 (commits `ec59277b` / `3756c84e` / `23452385` / `3b220f75` / `aeb85dd5`); 阶段 5 (凭据弹窗按"接入方式"分 tab + install URL 独立 QR) 本地完成待提交。iOS App 4B 用户已在独立仓库 `/Users/mark/MyProjects/iOSApp/UniClipboard` 落地并真机扫码测试通过。剩余:4C (SyncClipboard 快捷指令模板手工更新，仓库外)。

## 关键非目标 (本期不做)

- **不改 HTTP wire 协议**。SyncClipboard `GET /SyncClipboard.json` + Basic Auth 行为零变动。
- **不引入 HTTPS / TLS**。v1 仍是 LAN HTTP, 服务可达性 / 中间人由 LAN 信任前提兜底。
- **不实现 `o.token` / `o.exp`**。这些是 v2 演进方向 (协议规范 §10), v1 客户端仅"忽略未知键"实现前向兼容。
- **不替换 iCloud 快捷指令安装链接**。`SYNC_CLIPBOARD_EX_INSTALL_URL` 保留为"首次安装快捷指令"的次要入口，不删除。
- **不本仓库内维护 iOS 快捷指令模板**。模板属于产品资产，由独立仓库 / 工作流维护; 本仓库只产出说明文档。
- **不动 Android 客户端实现**。Android 兼容客户端由第三方实现，本仓库仅给字段映射文档。
- **不引入"扫码回执"接口**。密码轮换 / 撤销设备直接让旧 QR 在 Basic Auth 层失效，服务端不存 QR 状态。

## 已对齐的设计决策

1. **单一 scheme**: 仅接受 `uniclipboard://`, 不接受 `uniclip://` alias(简化 Intent filter / URL handler / 解析器逻辑)。
2. **base64url-no-pad 包裹 UTF-8 JSON**: 避免明文密码 / URL 特殊字符在 query string 中的二次编码问题，同时控制 QR 体积。
3. **JSON 字段固定顺序** (`v / url / user / pwd / o`) + **`o` 键 BTreeMap 字典序**: 保证 Rust 与 TS 编码器字节级一致，让 golden vector 可在两端复用。
4. **生成侧 `o` 字段白名单 + 解析侧宽松忽略未知键**: 编码侧用 `ConnectUriOther` 类型层强约束 (防 daemon bearer / 加密 passphrase 误塞); 解码侧用 `BTreeMap<String, String>` 接受任意键，前向兼容 v2 字段。
5. **`install_url` DTO 字段保留**: 短期内 iOS 首次引导仍要展示 iCloud 链接; 中期可考虑下沉为 `connect_uri.payload.o.install`。
6. **编解码模块归 `uc-application`**: 它服务于具体 use case, 且 payload schema 属应用层契约 (非领域真相)。
7. **URI 长度上限 800 字符**: 易扫描 + 防 `o` 滥用; build 路径硬性 sanity check, parse 路径无限制。
8. **MissingField 归并语义**: serde struct 字段加 `#[serde(default)]` 让"缺失"和"空字符串"统一翻译为 `MissingField`, 与规范 §4.2 错误码表对齐。

## 阶段总览

```
阶段 0 (协议规范文档)              ✅ ec59277b
    ↓
阶段 1 (Rust 编解码 + golden)      ✅ ec59277b
    ↓
阶段 2 (Rust use case + DTO)       ✅ 3756c84e
    ↓
阶段 3A (TS 解析器 + Vitest)       ✅ 23452385
    ↓
阶段 3B (凭据弹窗 UI)              ✅ 3b220f75
    ↓
阶段 4A (iOS App + Shortcut 文档)  ✅ aeb85dd5
    ↓
阶段 4B (iOS App 仓库 Swift 实现)  ✅ 跨仓库, 用户已落地并真机测试通过
    ↓
阶段 5  (凭据弹窗 Tab 重构)        ✅ 本地完成(待提交)
    ↓
阶段 4C (快捷指令模板 + iCloud)    ⏳ 仓库外手工,待用户落地
```

阶段 0-1 已合并提交。阶段 2-4 建议每阶段独立 PR, 行为非破坏 (老 iCloud 链接保留，前端阶段 2 还没改 → 现网无变化), 便于灰度。

---

## 阶段 0: 协议规范单一真相 ✅

**产出**: `docs/architecture/mobile-sync-connect-uri.md`

**关键内容**:
- §1 背景动机
- §2 URI 形态 (单 scheme + 800 字符上限)
- §3 v1 payload schema(字段表 + `o` 白名单 + 字节稳定性约定 + 双版本号)
- §4 解析算法 (mermaid 流程图 + 伪代码 + 错误码表)
- §5 安全约束 (明文密码 / 日志禁用 / 轮换语义)
- §6 SyncClipboard 字段映射
- §7 Golden test vector(1 happy + 6 负例，已实算验证)
- §8 端到端 onboarding 序列图
- §9 客户端集成说明 (iOS / Android / 未来原生)
- §10 v2 演进预案 (`o.exp` / `o.token`)
- §11 各阶段实现位置一览

**完成时间**: 2026-05-18 (commit `ec59277b`)

---

## 阶段 1: Rust 编解码纯函数 ✅

**产出**:
- `src-tauri/crates/uc-application/src/usecases/mobile_sync/connect_uri.rs`
- `src-tauri/crates/uc-application/src/usecases/mobile_sync/mod.rs` 注册一行

**关键内容**:
- `build_mobile_sync_connect_uri(base_url, username, password, other) -> Result<String, ConnectUriError>`
- `parse_mobile_sync_connect_uri(qr_text) -> Result<ConnectPayload, ConnectUriError>`
- `ConnectPayload` (反序列化目标，字段顺序 v/url/user/pwd/o, `o` 为 BTreeMap)
- `ConnectUriOther` (build 侧白名单 struct: label/did/proto/install)
- `ConnectUriError` (7 个错误变体，与规范 §4.2 + UriTooLong 自检)
- 22 个单元测试：golden 字节级匹配 / 全 6 负例 / alias 拒绝 / payload v 失配 / 未知 `o.*` 键宽松 / round-trip 含 Unicode label

**完成时间**: 2026-05-18 (commit `ec59277b`)

---

## 阶段 2: 桌面端 QR 内容切换 + DTO 调整 ✅

**产出**(本地，待提交):
- `src-tauri/crates/uc-application/src/usecases/mobile_sync/register_device.rs`
  - `RegisterMobileShortcutDeviceOutput` 加 `pub connect_uri: String`(install_url 保留)
  - `execute()` 在 device save + analytics emit 之后：
    - 组装 `ConnectUriOther { label, did:device_id, proto:"syncclipboard", install:None }`
    - `build_mobile_sync_connect_uri(&base_url, &username, &password, other)` → `translate_connect_uri_error` → `render_qr_code(&connect_uri)`
  - 新增 `translate_connect_uri_error()` helper: `UriTooLong→QrRenderFailed(带 len/max)`; 其余 6 个变体走 `unexpected:` catch-all(不可能触发，但保留诊断信息)
  - `render_install_qr` 重命名为 `render_qr_code`(只有一个调用方，crate 内零外溢)
- `src-tauri/crates/uc-application/src/usecases/mobile_sync/connect_uri.rs`
  - `parse_mobile_sync_connect_uri` + 3 个 parse-only 变体加 `#[allow(dead_code)]` 注释 (明确"测试 + 跨语言契约 + 未来 v2 daemon 接收侧"意图)
- `src-tauri/crates/uc-tauri/src/commands/mobile_sync.rs`
  - `RegisterMobileDeviceResult` 加 `pub connect_uri: String`(specta::Type + serde camelCase 自动透出 `connectUri`)
  - `From<Output>` 透传 + 2 个 DTO 单测
- `src/lib/ipc-bindings.generated.ts` 自动重生，新增 `connectUri: string` 字段 + doc-comment

**测试**:
- `register_device.rs` tests: 24 个 (22 旧 + 2 新): connect_uri prefix + parse 回 url/user/pwd + label/did/proto; QR 字节 ≠ install_url 编码
- 翻译函数直测：`UriTooLong` + 6 个 catch-all 变体逐一断言
- `uc-application` lib 全测：529 OK
- `uc-tauri` lib 全测：35 OK; mobile_sync DTO 单测：10 OK; specta_export: 1 OK
- 顺手消除 phase 1 留下的 10 个 dead-code 警告

**完成时间**: 2026-05-18 (本地)

---

## 阶段 3: 前端 TS 解析器 + 凭据弹窗 UI ⏳

### 3A: 共享解析器 ✅

**产出** (本地，待提交):
- `src/lib/mobileSyncConnectUri.ts` (303 行)
  - `buildConnectUri(baseUrl, user, pwd, other)` / `parseConnectUri(qrText)` 纯函数对
  - `ConnectUriError extends Error` + `ConnectUriErrorCode` 7 元联合，携带 `field/len/max/detail` 结构化字段
  - 6 条字节稳定性约束逐项实现，跨语言字节级一致
- `src/lib/__tests__/mobileSyncConnectUri.test.ts` (23 测试):
  - happy-path golden URI 字节级 === Rust 端字面量
  - 空 `o` 跳过 + `o` 键字典序强制
  - 5 build 负例 + 6 §7.2 parse 负例 + 4 边界负例
  - 前向兼容未知 `o.future_key`
  - build→parse round-trip 含 Unicode label

**测试**: `bun run test` 22 通过; `bun run test --run` 全套 80 文件 / 511 OK 无回归。

**完成时间**: 2026-05-18 (本地)

### 3B: 凭据弹窗 UI ✅

**目标**: 把主二维码语义切换到 connect URI (后端 DTO 已切),把 iCloud 安装链接降级为"首次安装"次要入口。

**产出** (本地，待提交):

1. `src/components/device/MobileSyncCredentialModal.tsx`:
   - iOS tab 主 QR 区图源不变 (后端阶段 2 已切到编码 connectUri), 文案换为 "扫码自动填凭据" + help 副文案。
   - 新增二级"首次安装"卡片包裹 install URL 字段; 沿用 CredentialField 自带 copy (桌面端打开 iCloud 链接无意义，不放 Open CTA)。
   - 顶部组件注释更新 iOS tab 描述：主操作 = connect URI QR, 次要 = 首次安装。

2. `src/i18n/locales/{en-US,zh-CN}.json`:
   - 改 `qr.label` / `qr.alt` → auto-fill 语义。
   - 新增 `qr.help` 副文案 + `installShortcut.{title,body}` 卡片文案。
   - `installUrl.label` 重命名为 "Install link (one-time)" / "安装链接 (一次性)"。

3. `src/components/device/__tests__/MobileSyncCredentialModal.test.tsx`:
   - mockPayload 加 `connectUri` 字段 (DTO 自 阶段 2 已有 → 之前测试缺字段是 TS 隐式 any tolerated)。
   - 新增 2 条断言：① QR alt = auto-fill + src 来自 PNG base64; ② 首次安装卡片标题 + install URL + install link label 三件套都可见。

**测试**:
- `bun run test src/components/device/__tests__/MobileSyncCredentialModal.test.tsx` → 9 通过 (原 7 + 新 2)
- `bun run test --run`(全套) → 80 文件 / 513 通过 (原 511 + 新 2), 无回归
- `npx eslint <两个改动 tsx>` → 0 error / 0 warning

**完成时间**: 2026-05-18 (本地)

---

## 阶段 4: iOS App 集成 + Shortcut 兜底 + 真机闭环 ⏳

**重要修订** (2026-05-18): 用户澄清主交互流程是"系统相机扫码 → 跳转 UniClipboard 原生 iOS App → 自动添加服务端",**不是** 快捷指令。已对齐 spec §9.1 提升为主路径，§9.2 (Shortcut) 降为仍维护的兜底路径。Android 文档暂不写 (spec §9.3 已够第三方实现参考)。

iOS 原生 App 在独立仓库 `/Users/mark/MyProjects/iOSApp/UniClipboard` 维护，本桌面仓库只产出集成文档，Swift 落地由用户在 iOS App 仓库完成。

### 4A: 集成文档 ✅

**产出** (本地，待提交):

1. `docs/integrations/ios-app-connect-uri.md` (新增，英文，~430 行):
   - §1 Why: 三条客户端路径对比 (系统相机 / 内嵌扫码 / Shortcut),解释 iOS App 现有 ServerQRPayload (JSON/URL-userinfo) 与 connect URI 的差异。
   - §2 URL scheme 注册：Xcode 26 generated-Info.plist 模型下，CFBundleURLTypes 必须走 Project → Target → Info → URL Types UI (无 INFOPLIST_KEY_* 等价); plutil 验证命令。
   - §3 .onOpenURL: 挂在 UniClipboardApp `WindowGroup` 上 (而非 ContentView), 防 SetupFlow / TabView 状态切换丢消息。AppViewModel.handleIncomingURL 路由 Sketch。
   - §4 Swift parser: `ConnectURI.Payload / ParseError` 类型 + 6 步 parser 完整 Swift 代码 (base64url decode helper, 字段提取，URL scheme 校验); 与 Rust/TS 行为对齐; 故意不实现 encoder (desktop 是唯一颁发方)。
   - §5 跨语言 golden test: 复用 spec §7.1 字面值 + 4 个 §7.2 负例，swift-testing macro 写法。
   - §6 UX 路由表：SetupFlow 空态 / Home tab 已有 server / 模态已开 三种状态分别该怎么响应。
   - §7 错误码 → i18n 文案映射建议 (en + zh-Hans)。
   - §8 ServerQRPayload.parse 入口统一改造 (legacy JSON/userinfo 保留，新增 connect URI 分支)。
   - §9 simctl openurl 验证命令 (含 §7.1 golden URI 字面值)。
   - §10 维护：o.* 新键无需 iOS 改动; v bump 需要协调; golden test 是 drift detector。

2. `docs/integrations/ios-shortcut.md` (新增，英文，~150 行):
   - §1 Why: 兜底定位 (用户未装 App 的回退路径，iCloud install URL 由 desktop 凭据弹窗的二级卡片露出)。
   - §2 两阶段 UX (不变)。
   - §3 详细 Shortcut Actions 表 (22 步，含 Receive Input / If / Match Text / Replace Text / Calculate padding / Base64 Decode / Get Dictionary 等), 标注每步类型 + 配置 + 变量绑定; 关键 base64url → base64 6 步映射 (- → +, _ → /, 重补 padding) 与 encoder 行为字节一致。
   - §4 用 spec §7.1 golden URI 自检 + 故障定位 checklist (group index / 变量级联 / padding off-by-one)。
   - §5 iCloud 分享指南 + `SYNC_CLIPBOARD_EX_INSTALL_URL` 常量更新触发条件。
   - §6 退役条件 (>95% 走原生 App 后)。

3. `docs/architecture/mobile-sync-connect-uri.md` 调整：
   - §9 整体重排为 "delivery-priority order": §9.1 改为"Native UniClipboard iOS App (primary)", §9.2 改为"SyncClipboard Shortcut template (fallback, still maintained)", §9.3 改为"Android / other third-party clients (spec is the contract, no per-client guide)"。
   - §11 实现位置表新增两行：ios-app-connect-uri.md + ios-shortcut.md。

**测试**: 文档型改动，无代码 / 单测变化。spec mermaid 图 + 既有跨语言 golden 测试 (Rust/TS) 已经覆盖协议层正确性，4A 不再重复测试。

**完成时间**: 2026-05-18 (本地)

### 4B: iOS App 仓库 Swift 落地 ⏳ (跨仓库)

**目标**: 在 `/Users/mark/MyProjects/iOSApp/UniClipboard` 按 4A 文档实现 connect URI 接入。

**预期改动** (在 iOS App 仓库，**非本桌面仓库**):
- `Shared/Network/ConnectURI.swift` (新增): Payload + ParseError + parse 函数。
- `Tests/UniClipboardNetworkTests/ConnectURITests.swift` (新增): golden vector + 6 个负例。
- `UniClipboard/UniClipboardApp.swift`: `.onOpenURL { vm.handleIncomingURL($0) }`。
- `UniClipboard/AppViewModel.swift`: 加 `handleIncomingURL` + present 方法。
- `UniClipboard/Views/QRScannerView.swift` (`ServerQRPayload.parse` 入口): 新增 `uniclipboard://` 分支。
- Xcode 项目：Target Info → URL Types + `uniclipboard` scheme。
- `Localizable.xcstrings`: 6 个错误码 i18n key。

**依赖**: 4A 文档 (作为实现规约)。

### 4C: 快捷指令模板 + iCloud 更新 ⏳ (仓库外手工)

**目标**: 把 4A-bis (ios-shortcut.md) §3 列出的 22 个 actions 实际加到 SyncClipboard "Clipboard EX" 模板中。

**手工步骤** (用户在 macOS / iPhone Shortcuts 编辑器操作):
1. 打开模板，按 ios-shortcut.md §3 表格依次加 actions，并把原"手输三栏"步骤改为 Otherwise 分支。
2. 用 §4 golden URI 自测，确认 BaseURL / User / Pwd 三个变量值正确。
3. Share → iCloud Link → 如新链接 ≠ `SYNC_CLIPBOARD_EX_INSTALL_URL`,本桌面仓库另开 PR 改常量。
4. 真机 iPhone(iOS 17+) UAT: 卸装重装模板 → 走新分支 + 走旧分支两条路径都通。

**依赖**: 4A 文档 (作为操作规约)。

### 验收 (4B + 4C 完成后)

- 真机 iPhone 系统相机扫桌面 QR → 弹"在 UniClipboard 打开" → tap → App 解析 connect URI → ServerForm prefill → 用户确认保存 → 触发同步 → desktop entry 列表出现新增项。 **(4B 已通过)**
- 备用路径：同一 QR 喂给 Shortcut 模板 → keychain 三栏自动填 → 后续 SyncClipboard 轮询正常。**(4C 待落地)**

---

## 阶段 5: 凭据弹窗按"接入方式"分 Tab + install URL 独立 QR ✅

**触发**: 用户在 4B 真机测试通过后提出 — 当前桌面端弹窗的 Tabs (iOS / Android) 是按平台分，但 connect URI QR 平台无关 (iOS App、SyncClipboard 客户端、Android 第三方应用都能解),"Android" tab 当前只显示一段"用第三方应用"文字、无 QR，既不准确也容易让 Android 用户误以为不支持。把 Tabs 改成按"接入方式"分，主路径 (扫码接入) 与兜底 (安装快捷指令) 各自独立。同时把 install URL 升级为独立 QR — iPhone 相机直扫即可装，不再要求用户在桌面上肉眼抄长 iCloud 链接到 Safari。

**改动文件** (本地，待提交):

1. **后端** `src-tauri/crates/uc-application/src/usecases/mobile_sync/register_device.rs`:
   - `RegisterMobileShortcutDeviceOutput` 新增 `install_qr_code_png_bytes: Vec<u8>` 字段，与既有 `qr_code_png_bytes` (编 connect URI) 对称。
   - `execute()` 走第二次 `render_qr_code(&install_url)`, 复用同一渲染管线 — 防止两个 QR 视觉/编码不一致。ASCII 不渲染 (CLI 用例不展示 install QR)。
   - 测试更新：
     - `auto_path_returns_minter_credentials_and_install_url` 加 `install_qr_code_png_bytes` PNG magic 断言。
     - `qr_content_follows_connect_uri_not_install_url` 加对称断言 — 主 QR ≠ install URL QR (旧), install QR == install URL QR (新，防字段串位)。

2. **Tauri DTO** `src-tauri/crates/uc-tauri/src/commands/mobile_sync.rs`:
   - `RegisterMobileDeviceResult` 新增 `install_qr_code_png_base64: String`, 走 specta::Type + camelCase 自动透出 `installQrCodePngBase64`。
   - `From<Output>` 透传 `BASE64.encode(out.install_qr_code_png_bytes)`。
   - 2 个 DTO 单测更新：`register_result_qr_is_base64_encoded` 加 install QR base64 断言; `register_result_serializes_connect_uri_camel_case` 加 wire 上 `"installQrCodePngBase64"` camelCase 断言。
   - `specta_export` 重生 `src/lib/ipc-bindings.generated.ts`, 自动多出字段 + doc-comment。

3. **前端 i18n** `src/i18n/locales/{en-US,zh-CN}.json`:
   - 删旧 keys: `platforms.{ios,android}` / `qr.{label,alt,help}` (顶层) / `installShortcut.{title,body}` / `installUrl.label` / `android.instructions`。
   - 新增结构化分组：
     - `platforms.{scan,shortcut}` — 新 Tab 标签 ("扫码接入" / "安装快捷指令", "Scan to add" / "Install Shortcut")
     - `scan.qr.{label,alt,help}` — Tab A 主 QR 文案，help 文案讲清"iOS App + Android 第三方应用都能扫"
     - `shortcut.{title,body}` — Tab B 顶部说明卡
     - `shortcut.qr.{label,alt}` — Tab B install URL QR 文案
     - `shortcut.linkLabel` — install URL CredentialField label

4. **前端 UI** `src/components/device/MobileSyncCredentialModal.tsx`:
   - `Platform = 'ios' | 'android'` → `OnboardingTab = 'scan' | 'shortcut'`, 默认 `scan`。`resetLocalState` 同步改。
   - Tab A "扫码接入": 主 connect URI QR (img src 不变，文案改 i18n) + 副 help 文案 — 即原 iOS tab 主 QR 区，但取消了次要"首次安装"卡片。
   - Tab B "安装快捷指令": 顶部说明卡 (title + body) + install URL QR (img src 走新 `payload.installQrCodePngBase64`) + CredentialField 显示 install URL 文本 (含 copy 按钮)。
   - 顶部组件注释重写：从"按平台分 tab"叙述切到"按接入方式分 tab", 明确两个 QR 内容 + 用途。

5. **前端单测** `src/components/device/__tests__/MobileSyncCredentialModal.test.tsx`:
   - mockPayload 加 `installQrCodePngBase64: 'aW5zdGFsbFFy'` (与 `qrCodePngBase64: 'iVBORw0KGgo='` 故意不同 — 防字段串位测试)。
   - 替换原 3B 加的 2 条断言：
     - `defaults to the Scan tab with the connect-URI QR`: 断 tab triggers 两个 label 都在，默认渲染 connect URI QR (src 来自 `qrCodePngBase64`), 新 help 文案 (含 Android 兼容客户端说明) 可见。
     - `switches to the Install Shortcut tab and shows the install-URL QR + link`: click Tab B → 顶部说明卡 + linkLabel + install URL 文本 + install QR (src 来自 `installQrCodePngBase64`, 与主 QR 互不串位) 都可见。
   - 现有 7 条 close-behavior 测试 (Done / Discard / Escape / acknowledgement / null payload / 双击 X) 全部保留，行为零变化。

**测试**:
- `cargo test -p uc-application --lib`: 529 OK (含 register_device 24 + connect_uri 22)
- `cargo test -p uc-tauri --lib`: 35 OK
- `cargo test -p uc-tauri --lib commands::mobile_sync`: 18 OK
- `cargo test -p uc-tauri --test specta_export`: 1 OK (bindings 写盘成功)
- `bun run test src/components/device/__tests__/MobileSyncCredentialModal.test.tsx`: 9 OK (原 7 + 新 2 替换原 2)
- `bun run test --run`: 80 文件 / 513 OK, 无回归
- `npx eslint <两个改动 tsx>`: 0 error / 0 warning

**完成时间**: 2026-05-18 (本地)

---

## 错误日志

(暂无)

## 决策日志

- 2026-05-18: 三个开放问题用户裁定
  1. 编解码模块归 `uc-application` (非 `uc-core`)
  2. `o` 字段采用"生成侧白名单 + 解析侧宽松"
  3. `install_url` DTO 字段保留
- 2026-05-18: 单一 scheme 决定 — 仅 `uniclipboard://`, 拒绝 `uniclip://` alias。简化 Intent filter / 解析器逻辑，避免客户端分级。
- 2026-05-18: `MissingField` 错误码归并语义 — serde struct 字段加 `#[serde(default)]`, 让"字段缺失"和"空字符串"统一翻译为 `MissingField`, 与规范 §4.2 错误码表对齐。
- 2026-05-18: golden vector 选用 `proto`/`label`/`did` 三个 `o` 键、不含 `install`, URI 长度 259 字符 (远低于 800)。
- 2026-05-18 (阶段 2): `o.install` 字段在阶段 2 暂留空，等阶段 4 真机走通后再决定是否塞 iCloud 链接到 payload。
- 2026-05-18 (阶段 2): `render_install_qr` 改名为 `render_qr_code` — 函数语义变了 (渲染任意 URI), 旧名误导。
- 2026-05-18 (阶段 2): `ConnectUriError` 全部翻译到 `QrRenderFailed`(复用既有变体，不新增错误码污染调用方); `UriTooLong` 带 `len/max` 提示，其余 catch-all 前缀 `unexpected:` 供日志排障。
- 2026-05-18 (阶段 2): parse 函数 + 3 个 parse-only 变体显式 `#[allow(dead_code)]` 而非删除，注释指明保留意图 (单测 / 跨语言契约 / 未来 v2 daemon 接收侧)。
- 2026-05-18 (阶段 3A): TS 端 `ConnectUriError` 用 `class ... extends Error` + `code: ConnectUriErrorCode` 联合常量，而非 discriminated union object —— JS 生态期望异常通道，且 class 能保留 stack trace, `code` 字段方便 i18n key 映射 (`CONNECT_URI_INVALID_SCHEME` 等)。
- 2026-05-18 (阶段 3A): JSON.stringify 字段顺序依赖浏览器 V8/JSC 的"字符串键按插入顺序"行为 (ES2015 起规范保证); 编码侧手动按 v/url/user/pwd/o 顺序构造对象，`o` 内部键先 sort 再插入 —— 与 Rust BTreeMap 字典序合起来保证字节级一致。
- 2026-05-18 (阶段 3A): `coercePayload` 对 `JSON.parse` 出的 `unknown` 显式 narrow, 非 string 的 `o.*` 键静默丢弃 (不污染 `Record<string, string>` 调用方契约); `v` 非整数走 UNSUPPORTED_VERSION 而非 PAYLOAD_DECODE_FAILED, 与 Rust serde u32 反序列化语义对齐。
- 2026-05-18 (阶段 3B): QR `<img src>` 字段保持 `data:image/png;base64,${qrCodePngBase64}` —— 后端 DTO 在阶段 2 已将该字段所编码的 URI 从 `installUrl` 切到 `connectUri`,前端无需感知具体编了什么，只需更新 alt/label 文案让 UX 语义对齐。
- 2026-05-18 (阶段 3B): 不为 install URL 加 "Open in Shortcuts" CTA —— 桌面端打开 iCloud 链接无意义，沿用 CredentialField 自带 copy (在 iPhone Safari 粘贴即可); 同时 `installShortcut.cta` i18n 文案删除以免成为孤儿键。
- 2026-05-18 (阶段 3B): 测试 mockPayload 加 `connectUri` 字段补齐 DTO; 不在前端单测里跑跨语言 byte-level 比对 (那是阶段 3A 的 mobileSyncConnectUri.test.ts 职责); 这里只断言 UI 结构 (alt 文案 + 次要卡片可见性),防止误改 UX。
- 2026-05-18 (阶段 4 范围澄清): 用户裁定 iOS 原生 App (走 URL scheme + .onOpenURL) 是主路径，SyncClipboard 快捷指令降为兜底但仍维护; Android 文档不写 (spec §9.3 已够第三方实现)。iOS App 在独立仓库 `/Users/mark/MyProjects/iOSApp/UniClipboard` 维护，本桌面仓库阶段 4A 只产文档，Swift 落地 (4B) 跨仓库，模板更新 (4C) 仓库外手工。
- 2026-05-18 (阶段 4A): iOS App 既有 `ServerQRPayload` (JSON / URL-userinfo 两种格式) 与 connect URI 不兼容 — 决策保留 legacy 格式，在 `ServerQRPayload.parse` 入口新增 `uniclipboard://` 分支调 ConnectURI.parse，让旧 QR 在 App 内嵌扫码下仍可用、新 QR 同时支持系统相机和 App 内嵌两个入口。
- 2026-05-18 (阶段 4A): iOS App Swift parser 故意不实现 encoder — desktop 是唯一 QR 颁发方，iOS 侧 encoder 是 dead code + drift 风险。golden test 的 byte-equality 由 desktop Rust/TS + iOS Swift 三方独立断言同一字面值实现，任一漂移立即可见。
- 2026-05-18 (阶段 4A): URL scheme 注册走 Xcode UI (Target → Info → URL Types) 而非 INFOPLIST_KEY_* — Xcode 26 的 INFOPLIST_KEY_* 不支持 `array<dict>` 结构，而 PlistBuddy build phase (类 UIFileSharingEnabled workaround) 是更重的备选，UI 路径足够稳定。
- 2026-05-18 (阶段 4A): .onOpenURL 挂在 `UniClipboardApp` 的 `WindowGroup` 根 (而非 ContentView) — 防 SetupFlow / TabView 状态切换时 handler 重新挂载丢消息;handleIncomingURL 路由收敛到 AppViewModel 一处。
- 2026-05-18 (阶段 5 触发): 4B 真机测试通过后用户提出 Tab 重构 — 按"平台 (iOS/Android)"分既不准确 (connect URI 平台无关), 也让 Android 用户看到一个空 tab 误以为不支持。改成按"接入方式 (扫码接入/安装快捷指令)"分，同时把 install URL 升级为独立 QR (iPhone 相机直扫装快捷指令), 不再要求肉眼抄长 iCloud 链接。
- 2026-05-18 (阶段 5): install URL QR 走后端 `render_qr_code` 二次渲染 (而非前端 qrcode 库即时生成) — 与 connect URI QR 共用同一管线，保证两个 QR 视觉/编码风格一致，同时前端零新依赖。ASCII 不渲染 (CLI 用例不需要 install QR)。
- 2026-05-18 (阶段 5): mockPayload `installQrCodePngBase64` 与 `qrCodePngBase64` 故意用不同 base64 字面值 — 单测断言两个 QR 的 img.src 各自指向各自的 base64, 防止前端把字段串位 (变量名相似，类型相同，复制粘贴失误是真实风险)。后端测试也加了对称断言 (install QR == install URL 编码，主 QR ≠ install URL 编码)。
- 2026-05-18 (阶段 5): Tab labels 采用"动作描述"而非"协议名" ("扫码接入"/"安装快捷指令" vs "Connect URI"/"iOS Shortcut") — 用户视角更直观，普通用户不必懂底层协议名。
