//! `StagedFile` —— 通过 [`MobileFileStagingPort`] 把 mobile 入站裸字节物
//! 化到本机文件系统后产出的"已 staging 文件"领域引用。
//!
//! 只承载业务真相的两件信息:
//! - `uri`:`file:///...` 形态的本地文件 URI(用于拼 file-list rep 的 wire
//!   bytes,跨平台格式由 adapter 负责);
//! - `sanitized_name`:adapter 安全化后的文件 basename(由 iPhone 上传时
//!   的 `dataName` 经 sanitize 而来,可能带容器目录信息已剥离)。
//!
//! 不暴露 `std::path::PathBuf` / `url::Url` —— 那些是 adapter 内部技术细
//! 节,uc-core 看不到也不应该看到。下游 use case 只需要 URI 字符串就能拼
//! file-list rep。
//!
//! [`MobileFileStagingPort`]: crate::ports::mobile_sync::MobileFileStagingPort

/// `file:///...` 形态的本地 URI,域中性 wrapper。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StagedFileUri(String);

impl StagedFileUri {
    /// 构造一个 URI 值对象。调用方(adapter)负责保证 `uri` 真的形如
    /// `file:///...`;本类型仅在域层做业务真相 wrapper,不再校验。
    pub fn new(uri: impl Into<String>) -> Self {
        Self(uri.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for StagedFileUri {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// adapter 写完字节后返回的"已 staging 文件"信息。
#[derive(Debug, Clone)]
pub struct StagedFile {
    /// 本机文件 URI,可直接拼进 `text/uri-list` rep。
    pub uri: StagedFileUri,
    /// adapter sanitize 后的实际文件 basename(去掉路径分隔符 / `..` 等)。
    /// use case 不直接消费它,但保留在返回值里方便日志 / 排障。
    pub sanitized_name: String,
}
