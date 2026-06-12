# Spike 方案（收窄版）：证明 Rust→iOS/Android FFI 管道

> 配套 `uc-ios-feature-inventory.md`（基线）+ `uc-ios-regression-checklist.md`（全量迁移的验收闸门）。
> 本版按对抗审查 `wkkg9l3cg`（34 条确认）收窄重写，取代旧版「全量迁移」框架。
> 状态：spike / B0–B2 完成（管道证明成立）。语言审查豁免路径（`.planning/`）。
> 进度：B0 ✅（`uc-mobile-proto` 抽出，commit c1576bd05）· B1 ✅（`uc-mobile` UniFFI crate + xcframework + Swift binding，iOS 模拟器 demo 三探针全过：golden vector 解析 / 错误映射 / `with_foreign` bridge 构造 + 回调往返）· B2 ✅（`uc_mobile_init` ring provider + async `get_latest`/`put_clipboard`/`tls_probe`，模拟器 demo 对真实 daemon 完成 put+get 往返、401 映射、真实 TLS 握手；缝 3 由 detached-task 执行模型落实并有 drop/cancel 单测；编排脚本 `crates/uc-mobile/scripts/run-b2-daemon-demo.sh`）
> ⚠️ B2 验收中「App / 键盘扩展 / 分享扩展三进程上下文各自 TLS 握手」**未在 spike 内验证**——demo 载体是单进程 CLI 二进制，三进程验收需等接入真实 uc-ios app（目标 B 启动时补）。其余 DoD 全部达成。

## 0. 一句话定位

**这个 spike 只做一件事：证明「Rust 逻辑能通过 async FFI 被 iOS/Android 调用、打通真实网络」这条管道能跑通，为未来 P2P（iroh，Rust-only，无法原生重写）铺轨。**

mobile-sync 只是低风险载体。**不在本 spike 范围**：零回归地把整个 mobile-sync 搬进 Rust。

### 为什么这样切（关键认知）

旧版把两个目标焊死了：
- **目标 A（本 spike）**：证明 FFI 管道 → 工作量小、风险可控。
- **目标 B（后续独立项目）**：零回归全量迁移 mobile-sync → 工作量大一个数量级。

对抗审查的 14 个 major 几乎全在打目标 B（SyncEngine、扩展 uploader、HistoryRecord/multipart/PATCH/grapheme 等 byte-critical 端口、split-brain）。**拆开两个目标后，这些 major 因出范围而消解**，本 spike 只剩 3 个必修工程缝。

---

## 1. 范围

### ✅ 本 spike 做（B0–B2）

| 步 | 内容 | 证明什么 |
|---|---|---|
| **B0** | 新建 `crates/uc-mobile-proto` 叶子 crate，把 `connect_uri.rs`（真零内部依赖）移入；`uc-application` 改为依赖它；桌面 build+test 全绿无回归 | 抽取拓扑成立 |
| **B1** | 新建 `crates/uc-mobile`（UniFFI），暴露 **同步** 纯函数 `parse_connect_uri`；用 `#[uniffi::export(with_foreign)]` 验证 `Arc<dyn PlatformBridge>` 构造参数；产出 xcframework + Swift binding，iOS demo 调通 | UniFFI codegen 管道通 + 构造参数写法可行 |
| **B2** | `uc-mobile` 加 **async** `get_latest`/`put_clipboard`（reqwest + tokio + `uc_mobile_init`），iOS demo 打 **真实桌面 daemon** 成功完成一次 get 和一次 put | **async-over-FFI + tokio on device + TLS 初始化**（P2P 真正的硬骨头） |

**DoD = B0–B2 完成，且 iOS 通过 async Rust 对真实 daemon 完成 get+put。就这一句。**

### 🚫 本 spike 明确不做（推迟到「目标 B：全量迁移」独立项目）

- 968 行 `SyncEngine` 状态机（tick/epoch/loop-guard/server-wins）
- 键盘/分享/App-Intents 的独立 uploader（各有不同 watermark 顺序）
- multipart builder、`HistoryRecord` 全套不变量（composite/split id、`isDelete`/`isDeleted`、version+409）、§2.9 create、§2.10 PATCH、长文本 grapheme 阈值
- `SyncLoopGuard`/`PayloadCache`/`ConnectionTester`/`ServerConfig` 迁移逻辑
- **`Transport` 抽象**——删除。其唯一理由（移动端 P2P=轻客户端）尚未由用户拍板、且与 VISION §63 冲突；等 P2P 决策落地再引入。
- **「零回归」不是本 spike 的验收标准**——它是目标 B 的标准。

---

## 2. Crate 拓扑

```
crates/
├── uc-mobile-proto   ← B0 新增·叶子（零重依赖）
│     当前只放 connect_uri（编解码 + ConnectPayload + ConnectUriError）
│     deps: base64, serde, thiserror, url, std —— 仅此
│     桌面 daemon（经 uc-application）与未来 uc-mobile 共依赖
│
└── uc-mobile         ← B1 新增·FFI 边界
      依赖 uc-mobile-proto + reqwest(ring-pinned rustls) + tokio + uniffi
      B1 只暴露同步纯函数；B2 加 async client + uc_mobile_init
```

### ⚠️ 诚实声明：golden vector 才是唯一跨语言契约

旧版宣称「同一份代码 → 桌面/移动端字节一致」。**这是错的**：
- connect-uri 实际有 **三份实现**——Rust `connect_uri.rs`、驱动桌面 QR 的 TS `src/lib/mobileSyncConnectUri.ts`、iOS `ConnectURI.swift`。B0 只减少 Rust 内部使用面，**消除不了跨语言漂移**。
- 决定 SyncClipboard JSON 字节的是 `uc-webserver/.../sync_doc.rs` 的 `SyncClipboardDoc`（`pub(super)`，server-only，带 rename/alias/nil 省略）；`uc-application` 的 `SyncClipboardMeta` **零 serde、不是 wire 类型**。

**结论**：本 spike 不靠「单一源头」保证字节一致，靠 **golden vector 作为跨实现契约**，且 golden vector 必须覆盖 Rust + TS + iOS 三方。

---

## 3. 三个必修工程缝（baked into B0–B2）

### 缝 1（原 blocker）：rustls CryptoProvider 无安装点
workspace 的 rustls 同时编入 aws-lc-rs + ring，无自动默认，必须显式 `install_default()`（见 `apps/cli/src/main.rs:311`、`apps/daemon/src/main.rs:56`）。FFI cdylib 被 App/键盘/分享扩展加载时 **没有 `main()`**。
- **修**：`uc-mobile` 导出 `uc_mobile_init()`，用 `OnceLock` 跑一次 `rustls::crypto::ring::default_provider().install_default()`；Swift/Kotlin 构造 client 前必须先调。
- **修**：reqwest `default-features=false`，pin ring（照 `apps/cli/Cargo.toml:69`、`uc-infra/Cargo.toml:121`）；CI 断言 mobile target 下 `cargo tree -i aws-lc-rs` 为空。
- **B2 验收**：App / 键盘扩展 / 分享扩展三个进程上下文各自首次 TLS 握手成功。

### 缝 2：UniFFI 构造参数写法
`#[uniffi::export(callback_interface)]` 的 trait 不能当 `Arc<dyn PlatformBridge>` 构造参数（uniffi-rs #2797）。
- **修**：改 `#[uniffi::export(with_foreign)]`；**B1 就验证** 这个写法能编译 + 生成 binding（它是整条管道论点的门，别拖到 B2）。
- 注：同步 PlatformBridge + Object 上 `async_runtime="tokio"` 的组合本身 UniFFI 支持（#2576 不命中）；但 ergonomics 仍需对 pinned uniffi 版本实测。

### 缝 3：async-PUT 被挂起打断
真正的风险不是「死锁」，是挂起期 PUT 被打断 → 本地 watermark 写了、远程 metadata 没写 = 损坏（`ShareUploader.swift` 现状无 rollback）。
- **修**：B2 验收从「不死锁」改写为「**被挂起打断的 PUT 留下可恢复而非损坏的状态**」+「file→metadata 窗口内 future drop 是原子或幂等」。加一个在两个 PUT 之间 drop future 的测试。
- 注：进程挂起拆掉 tokio runtime 是真正的中断源（非 Swift Task 取消传播）。

---

## 4. UniFFI 接口草图（B2，已修正）

```rust
// uc-mobile/src/lib.rs
#[uniffi::export]
fn uc_mobile_init();                                  // 缝 1：OnceLock install rustls ring provider

#[uniffi::export]
fn parse_connect_uri(uri: String) -> Result<ConnectPayload, ConnectUriError>;  // B1

#[uniffi::export(with_foreign)]                        // 缝 2：不是 callback_interface
pub trait PlatformBridge: Send + Sync {
    fn app_group_dir(&self) -> String;
    // I/O bridge：snapshot 模式——进 async 段前由原生读完字节传入，
    // 不在 async future 内同步回调阻塞 tokio worker（审查 FACT-3 feasibility）
}

#[derive(uniffi::Object)]
pub struct MobileSyncClient { /* reqwest(ring) + current_thread tokio + bridge */ }

#[uniffi::export(async_runtime = "tokio")]
impl MobileSyncClient {
    #[uniffi::constructor]
    fn new(bridge: Arc<dyn PlatformBridge>) -> Arc<Self>;
    async fn get_latest(&self, server: ServerConfig) -> Result<ClipboardMeta, SyncError>;  // §2.1
    async fn put_clipboard(&self, server: ServerConfig, doc: ClipboardMeta, payload: Option<Vec<u8>>) -> Result<(), SyncError>;  // §2.2/§2.3
    fn cancel_in_flight(&self);
}
```

> 扩展内存：tokio 强制 `Builder::new_current_thread().enable_all()`，reqwest 禁 idle pool（`pool_max_idle_per_host=0`）——iOS 扩展 ~48MB jetsam 上限，不能上多线程 runtime + 连接池（审查 FACT-2 feasibility）。

---

## 5. CI / 构建（B1–B2）

- pin：uniffi 版本、NDK 版本、android API level、rust toolchain（现 1.95.0）——B1 前全部钉死，否则管道不可复现。
- iOS：`cargo build` aarch64-apple-ios(+sim) → `xcodebuild -create-xcframework` → `UniClipboardCore.xcframework` + UniFFI Swift binding。
- Android（B2 可选延后）：cargo-ndk → `.so` + AAR + Kotlin binding。
- 新增 CI：交叉编译 + binding 生成 + 体积报告（硬性体积预算 fail 条件）+ `cargo tree -i aws-lc-rs` 为空断言。
- 先验证 ring 的 `aarch64-apple-ios` asm 构建通过，再谈下一步。

---

## 6. 风险与待决

- **假 oracle 警告（留给目标 B，现在记下）**：daemon 的 history query/PATCH 是 **兼容壳**（patch 不读 body、version 硬编码 0、无 409、无 modifiedAfter，见 `routes.rs:15-16`）——未来全量迁移 `HistoryRecord`/version/isDelete 时，**真实 daemon 不是可靠字节对照物**。本 spike 不碰这些，但目标 B 必须另找 oracle（抓 iOS 真实字节 fixture）。
- **✅ 已决（用户拍板 2026-06-12）：移动端只做 mobile-sync，不做真正的 P2P**。与 VISION §63「Mobile 走独立 LAN HTTP 协议」完全一致，VISION 无需改写；Transport 抽象继续不引入。`uc-mobile` 的定位从「P2P 铺轨」收敛为「mobile-sync 的共享 Rust 实现载体」——spike 证明的 FFI 管道照常是目标 B 的地基，P2P 论述仅作历史背景保留。
- **iOS demo 载体**：B1/B2 用一个最小 iOS demo target（非接入正式 uc-ios app），避免污染产品代码；管道证明后再谈接入。

---

## 7. Spike 完成定义（DoD）

1. B0：`uc-mobile-proto` 抽出，桌面 build+test 全绿无回归。
2. B1：iOS demo 经 UniFFI 调通同步 `parse_connect_uri`；`with_foreign` 构造参数写法编译通过；xcframework + Swift binding 产出。
3. B2：iOS demo 经 **async** Rust 对 **真实 daemon** 完成一次 get + 一次 put；三进程上下文 TLS 握手成功；挂起打断不致损坏。
4. 产出判断：**这条管道能否承载未来 P2P**——这才是 spike 的真正交付物。

> 目标 B（零回归全量迁移）是另一份方案，待管道证明后启动，按 `uc-ios-regression-checklist.md` 一个端口一个端口移植 + golden vector。
