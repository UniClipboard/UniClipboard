//! `.shortcut` 模板打包与二维码渲染服务。
//!
//! 这是 application 层的内部 service（不是 `uc-core` port）—— 它的实现
//! 是纯计算（plist DOM 替换 + qrcode 矩阵生成），没有外部 IO，按
//! `uc-application/AGENTS.md` §5 与 `docs/agent/architecture-rules.md`
//! "Implementation Order" 的指引应停留在 application 层。
//!
//! 之所以仍然抽象为 trait 而非裸 struct：
//!   1. v1 Phase 2 我们用 stub 实现，让 `RegisterMobileShortcutDeviceUseCase`
//!      先跑通；Phase 3 接入真实的 `plist` + `qrcode` crate 后，trait 边界
//!      不变。
//!   2. use case 的单元测试直接 mock 这个 trait，不需要构造真实 .shortcut
//!      文件。
//!
//! 输出由 `RenderedQr` / `PackedShortcut` 这种轻量 DTO 表达 —— 不要在这
//! 里把内部数据结构（plist::Value 等）泄露出去。

use thiserror::Error;

use uc_core::mobile_sync::MobileDeviceId;

/// 注入到 `.shortcut` 模板里的 3 个占位变量值。
///
/// 模板 plist 中的 `__UC_URL__` / `__UC_TOKEN__` / `__UC_DEVICE_ID__`
/// 字符串字面量会被这里的实参精确替换。详见 `.context/mobile-sync/SPEC.md`
/// §6.1。
#[derive(Debug, Clone)]
pub(crate) struct ShortcutPackParams {
    /// 形如 `http://192.168.1.5:42720`，写入 `.shortcut` 的 url 变量。
    pub lan_url: String,
    /// 64 字符 hex token，写入 `.shortcut` 的 token 变量。
    pub raw_token_hex: String,
    /// 服务端分配的设备 id，写入 `.shortcut` 的 deviceId 变量。
    pub device_id: MobileDeviceId,
}

/// 一次成功打包的产物：`.shortcut` 二进制 + 已渲染的二维码（PNG / ASCII
/// 双形态），方便上层 facade 直接转发给前端 / CLI 不再二次渲染。
#[derive(Debug, Clone)]
pub(crate) struct PackedShortcut {
    /// 未签名的 binary plist，可直接 HTTP 响应给 iPhone Safari。
    pub shortcut_bytes: Vec<u8>,
    /// 二维码 PNG 字节流，供前端通过 base64 数据 URL 渲染。
    pub qr_code_png_bytes: Vec<u8>,
    /// 二维码 ASCII（块字符），供 CLI 直接 `println!`。
    pub qr_code_ascii: String,
}

#[derive(Debug, Error)]
pub(crate) enum ShortcutPackError {
    /// 模板解析失败 —— 模板文件本身坏了，是发布时的问题，运行期无解。
    #[error("shortcut template is malformed: {0}")]
    MalformedTemplate(String),

    /// 模板里没找到必需的占位符 —— 同样属于发布期问题。
    #[error("shortcut template missing required placeholder: {0}")]
    MissingPlaceholder(&'static str),

    /// 二维码渲染失败 —— qrcode 库给的输入约束（典型：URL 过长）。
    #[error("qr code rendering failed: {0}")]
    QrRender(String),
}

/// 给 `RegisterMobileShortcutDeviceUseCase` 用的 service 抽象。
///
/// 实现位于同模块内（`StubShortcutPackerService`，Phase 2 占位）以及
/// 后续 Phase 3 引入的 `PlistShortcutPackerService`（真实 plist + qrcode
/// 实现）。
pub(crate) trait ShortcutPackerService: Send + Sync {
    /// 把 3 个变量注入模板并打包为 `.shortcut` 二进制；同时为给 iPhone 用
    /// 的下载 URL 渲染一份二维码（PNG + ASCII）。
    ///
    /// `download_url` 需要由调用方在注册了一次性下载 token 之后再传入；
    /// 它是出现在二维码里的内容，不是写进 `.shortcut` 里的内容（写进
    /// .shortcut 的是 lan_url + token）。
    fn pack(
        &self,
        params: &ShortcutPackParams,
        download_url: &str,
    ) -> Result<PackedShortcut, ShortcutPackError>;
}

// ─── Stub 实现（Phase 2 占位） ─────────────────────────────────────────

/// Phase 2 的占位实现：返回固定字节序列。
///
/// 目的是让上层 use case + facade 编译通过、单元测试可对它做 mock；
/// Phase 3 用真实的 plist DOM 替换 + qrcode 矩阵生成替换它。
///
/// 故意没有 `pub(crate) fn new` —— 所有调用都应通过 use case 单测里直接
/// `Arc::new(StubShortcutPackerService)`，不在生产代码里 wire 它。
pub(crate) struct StubShortcutPackerService;

impl ShortcutPackerService for StubShortcutPackerService {
    fn pack(
        &self,
        params: &ShortcutPackParams,
        download_url: &str,
    ) -> Result<PackedShortcut, ShortcutPackError> {
        // 返回的字节里嵌入参数 —— 单元测试可以据此断言"参数确实流到了
        // packer"，避免 use case 与 packer 之间出现哑参传递 bug。
        let placeholder = format!(
            "STUB_SHORTCUT|url={}|token={}|device={}",
            params.lan_url,
            params.raw_token_hex,
            params.device_id.as_str(),
        );
        Ok(PackedShortcut {
            shortcut_bytes: placeholder.into_bytes(),
            qr_code_png_bytes: format!("STUB_QR_PNG|{download_url}").into_bytes(),
            qr_code_ascii: format!("STUB_QR_ASCII|{download_url}"),
        })
    }
}
