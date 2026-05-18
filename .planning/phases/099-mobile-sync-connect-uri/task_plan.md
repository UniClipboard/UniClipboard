# Task Plan: 099 · 移动端扫码接入协议 `uniclipboard://connect`

## 目标

为移动端注册流程引入版本化深链协议 `uniclipboard://connect`,把 `base_url / username / password / 扩展元数据`编码进 **单个二维码**,让 iOS Shortcut / Android SyncClipboard 兼容客户端 / 未来原生 App **免输入接入**,消除 `MobileSyncCredentialModal` 中"用户肉眼抄写三栏"的体验缺陷。

跟踪 issue: <https://github.com/UniClipboard/UniClipboard/issues/789>

## 当前阶段

阶段 0-2 已提交 (commits `ec59277b` 与 `3756c84e`); 阶段 3A + 3B 本地完成待提交。下一步进入阶段 4 (iOS 模板 + Android 客户端文档)。

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
阶段 0 (协议规范文档)          ✅ ec59277b
    ↓
阶段 1 (Rust 编解码 + golden)  ✅ ec59277b
    ↓
阶段 2 (Rust use case + DTO)   ✅ 3756c84e
    ↓
阶段 3A (TS 解析器 + Vitest)   ✅ 本地完成(待提交)
    ↓
阶段 3B (凭据弹窗 UI)          ✅ 本地完成(待提交)
    ↓
阶段 4 (iOS 模板 + 客户端文档) ⏳ 待办
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

## 阶段 4: iOS 快捷指令模板 + Android 客户端文档 ⏳

**目标**: 关闭端到端闭环，让真机 iPhone 扫码即接入。

**改动文件**(本仓库范围):

1. `docs/integrations/ios-shortcut.md` (新增，英文)
   - 两阶段 UX 流程 (首次安装 → 后续扫码)
   - 快捷指令需新增的步骤详解：
     - URL Trigger 检测 `uniclipboard://` 前缀
     - 提取 `p` query param
     - base64url 解码 (Shortcut 内置 base64; 需 `+/` 与 `-_` 字符映射 + padding 补齐)
     - 字典取值 → 写入三个文本变量 (url/user/pwd)
   - 错误处理建议 (各错误码对应用户提示)

2. `docs/integrations/android-syncclipboard.md` (新增，英文，可选)
   - Intent filter 声明示例
   - 字段映射表 (`url` / `user` / `pwd` → SyncClipboard App 三个配置项)
   - 兼容客户端实现 checklist

**模板维护**(本仓库范围外):
- iOS 快捷指令模板更新 + 重新签名 + 通过 iCloud 分享 → 拿到新 iCloud 链接，**可能需要更新 `SYNC_CLIPBOARD_EX_INSTALL_URL` 常量**(取决于是否复用旧链接)

**验收**:
- 真机 iPhone(任一 iOS 17+) 一次扫码 → 快捷指令自动写入三栏 → 触发同步 → desktop 端 entry 列表出现新增项

**依赖**: 阶段 3 完成，有可扫的真实 QR 可用作 UAT。

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
