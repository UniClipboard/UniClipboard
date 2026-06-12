//! 中继节点诊断 port 与领域中性的结果/错误类型。
//!
//! 这是 application 层为"网络设置诊断"动作定义的内部 trait。它**不属于**
//! `uc-core` —— "测试某个 iroh relay 是否可达"是一个技术诊断动作,不是设备
//! 之间的领域关系,放进 core 会让 core 知道"relay"这个传输层概念(参见
//! `uc-core/AGENTS.md` §6.2)。
//!
//! 依赖方向:
//!
//! * application 定义 trait 与领域中性的结果类型;
//! * infra 不实现该 trait,只提供具体的探测能力(例如基于 iroh-relay 的
//!   adapter)与自己的内部错误类型;
//! * bootstrap 写 newtype adapter,把 infra 的具体能力转译成本 trait,
//!   注入到 [`SettingsFacade`](super::facade::SettingsFacade)。

use async_trait::async_trait;

/// 探测成功时的报告。字段语义与具体协议无关:`latency_ms` 是端到端往返
/// 耗时。未来如果需要暴露协议版本/服务端 ID,在此 struct 增字段并同步
/// infra/bootstrap 的 1:1 映射。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelayProbeReport {
    pub latency_ms: u32,
}

/// 应用层归类后的中继诊断错误。每个变体承诺一个稳定语义,上层据此挑选
/// 用户文案;具体实现内部的错误细节(例如 iroh-relay 的 `ConnectError`)
/// 不允许通过此类型泄漏到 application 之上的层。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RelayProbeError {
    #[error("invalid relay URL: {0}")]
    InvalidUrl(String),
    #[error("dns lookup failed: {0}")]
    Dns(String),
    #[error("tls handshake failed: {0}")]
    Tls(String),
    #[error("relay handshake failed: {0}")]
    Handshake(String),
    #[error("relay probe timed out")]
    Timeout,
    #[error("relay probe failed: {0}")]
    Other(String),
}

/// "对一个候选中继 URL 做一次可达性探测"的能力抽象。
///
/// 由具体实现负责:
///
/// * 不读取也不修改任何持久化状态,可以被反复调用;
/// * 在自行决定的预算时间内必须返回 [`RelayProbeError::Timeout`],不能让
///   调用方在不确定时长内阻塞;
/// * 不复用任何长期身份/凭据,避免向被测对端泄露应用内的稳定 ID。
#[async_trait]
pub trait RelayDiagnosticPort: Send + Sync {
    async fn probe(&self, url: &str) -> Result<RelayProbeReport, RelayProbeError>;
}
