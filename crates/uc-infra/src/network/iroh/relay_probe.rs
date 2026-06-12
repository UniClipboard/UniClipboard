//! 基于 iroh-relay 的中继可达性探测能力。
//!
//! 调用 [`iroh_relay::client::ClientBuilder::connect`] 走完整的 WebSocket
//! 升级 + 协议握手,把成功握手所耗时间作为延迟报告,把握手过程中的失败
//! 归类到下方 [`RelayProbeError`]。每次探测使用全新随机 [`SecretKey`],
//! 不复用进程内的长期 iroh 身份,避免向被测试中继泄露 NodeId。
//!
//! 本模块只提供 inherent 方法 —— **不实现任何 application 层 trait**。
//! application/bootstrap 负责把 [`IrohRelayProbeAdapter::probe`] 包装为
//! 上层依赖的 port,这条边界由 `uc-infra/AGENTS.md` §4.2 与 `uc-core/AGENTS.md`
//! §6.2 共同要求(iroh-relay 是 transport-layer 实现,不允许沿端口契约
//! 泄露到 core)。

use std::time::{Duration, Instant};

use iroh::dns::DnsResolver;
use iroh::{RelayUrl, SecretKey};
use iroh_relay::client::{ClientBuilder, ConnectError, DialError};
use iroh_relay::tls::{self, CaRootsConfig};
use tokio::time::error::Elapsed;
use tracing::{debug, instrument, warn};

/// 探测整体预算。覆盖 DNS + TCP + TLS + WebSocket upgrade + 协议握手;
/// 超过此预算返回 [`RelayProbeError::Timeout`]。
const PROBE_BUDGET: Duration = Duration::from_secs(5);

/// 探测成功时 infra 返回的中性报告。字段语义与上层 application 镜像一致,
/// 由 bootstrap adapter 做一次 1:1 转译。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelayProbeReport {
    pub latency_ms: u32,
}

/// infra 内部归类后的探测错误。每个变体语义稳定,bootstrap 直接 match 转
/// 到 application 错误集合;具体三方错误(`ConnectError` / `DialError` /
/// `DnsError` 等)被压成 `String`,不沿此类型泄漏出 infra crate 边界。
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

/// 使用 iroh-relay 官方 client 做协议级握手的探测器。
///
/// 持有可复用的 [`DnsResolver`] 与 TLS [`rustls::ClientConfig`],避免每次
/// 探测都重新初始化 crypto provider。
pub struct IrohRelayProbeAdapter {
    dns_resolver: DnsResolver,
    tls_config: rustls::ClientConfig,
}

impl IrohRelayProbeAdapter {
    /// 构造一个使用系统 DNS + 内嵌 webpki 根证书的探测器。
    pub fn new() -> Result<Self, RelayProbeError> {
        let crypto_provider = tls::default_provider();
        let tls_config = CaRootsConfig::embedded()
            .client_config(crypto_provider)
            .map_err(|err| RelayProbeError::Other(format!("init tls config: {err}")))?;
        Ok(Self {
            dns_resolver: DnsResolver::new(),
            tls_config,
        })
    }

    /// 对单个候选中继 URL 发起一次握手探测。
    ///
    /// 不读取也不修改任何持久化状态,可重复调用。在 [`PROBE_BUDGET`] 内必
    /// 须返回 [`RelayProbeError::Timeout`]。
    ///
    /// 注意:tracing span 仅记录 scheme + host(+port),丢弃 userinfo / path /
    /// query —— 防止用户误粘贴的 `https://user:token@...` 把凭据写进日志。
    #[instrument(skip(self, url), fields(relay = %sanitize_url_for_log(url)))]
    pub async fn probe(&self, url: &str) -> Result<RelayProbeReport, RelayProbeError> {
        let trimmed = url.trim();
        if trimmed.is_empty() {
            return Err(RelayProbeError::InvalidUrl(
                "relay URL must not be empty".to_string(),
            ));
        }
        let relay_url: RelayUrl = trimmed
            .parse()
            .map_err(|err| RelayProbeError::InvalidUrl(format!("{err}")))?;
        let scheme = relay_url.scheme();
        if scheme != "http" && scheme != "https" {
            return Err(RelayProbeError::InvalidUrl(format!(
                "scheme `{scheme}` is not supported; expected http or https"
            )));
        }
        if relay_url.host_str().is_none() {
            return Err(RelayProbeError::InvalidUrl(
                "relay URL must include a host".to_string(),
            ));
        }

        // 一次性凭据 —— 与长期 iroh endpoint 身份解耦,避免把进程级 NodeId
        // 泄露给被测试的中继。
        let secret = SecretKey::generate();
        let builder = ClientBuilder::new(relay_url, secret, self.dns_resolver.clone())
            .tls_client_config(self.tls_config.clone());

        let started_at = Instant::now();
        let connect = tokio::time::timeout(PROBE_BUDGET, builder.connect()).await;

        match connect {
            Ok(Ok(_client)) => {
                let latency_ms =
                    u32::try_from(started_at.elapsed().as_millis()).unwrap_or(u32::MAX);
                debug!(latency_ms, "relay probe succeeded");
                Ok(RelayProbeReport { latency_ms })
            }
            Ok(Err(err)) => Err(map_connect_error(err)),
            Err(Elapsed { .. }) => Err(RelayProbeError::Timeout),
        }
    }
}

/// 把任意输入压成 `scheme://host[:port]`,无法解析时返回 `<unparseable>`。
///
/// 仅用于 tracing 字段 —— 避免把 userinfo / path / query / fragment(可能含
/// token、session id 等敏感片段)落进日志。完整的原始 URL 仅在内存里参与
/// 协议握手,不会跨进程边界。
fn sanitize_url_for_log(url: &str) -> String {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return "<empty>".to_string();
    }
    url::Url::parse(trimmed)
        .ok()
        .and_then(|parsed| {
            let host = parsed.host_str()?;
            let scheme = parsed.scheme();
            Some(match parsed.port() {
                Some(port) => format!("{scheme}://{host}:{port}"),
                None => format!("{scheme}://{host}"),
            })
        })
        .unwrap_or_else(|| "<unparseable>".to_string())
}

fn map_connect_error(err: ConnectError) -> RelayProbeError {
    // `ConnectError` 与 `DialError` 用 n0-error 派生宏注入了 `meta` 字段,
    // 这里只关心可读语义,统一用 `..` 跳过 meta。
    match err {
        ConnectError::InvalidWebsocketUrl { url, .. } => {
            RelayProbeError::InvalidUrl(format!("invalid websocket URL: {url}"))
        }
        ConnectError::InvalidRelayUrl { url, .. } => {
            RelayProbeError::InvalidUrl(format!("invalid relay URL: {url}"))
        }
        ConnectError::Dial { source, .. } => map_dial_error(source),
        ConnectError::Tls { source, .. } => RelayProbeError::Tls(source.to_string()),
        ConnectError::InvalidTlsServername { .. } => {
            RelayProbeError::Tls("invalid TLS servername".to_string())
        }
        ConnectError::Handshake { source, .. } => RelayProbeError::Handshake(source.to_string()),
        ConnectError::BadVersionHeader { server_version, .. } => {
            RelayProbeError::Handshake(format!(
                "server replied with unsupported version `{}`",
                server_version.as_deref().unwrap_or("<empty>")
            ))
        }
        ConnectError::UnexpectedUpgradeStatus { code, .. } => {
            RelayProbeError::Handshake(format!("unexpected HTTP upgrade status: {code}"))
        }
        ConnectError::Upgrade { source, .. } => {
            RelayProbeError::Handshake(format!("http upgrade failed: {source}"))
        }
        ConnectError::Websocket { source, .. } => {
            RelayProbeError::Handshake(format!("websocket error: {source}"))
        }
        ConnectError::NoLocalAddr { .. } => {
            RelayProbeError::Other("no local socket address available".to_string())
        }
        ConnectError::MissingCryptoProvider { .. } => {
            RelayProbeError::Other("rustls crypto provider missing".to_string())
        }
        // 兜底分支:把陌生 ConnectError 变体压成 Other,同时 warn 保留源头便
        // 于排查(iroh-relay 升级新增变体时是这里第一时间发现)。
        other => {
            warn!(error = ?other, "relay probe: unmapped ConnectError variant");
            RelayProbeError::Other(other.to_string())
        }
    }
}

fn map_dial_error(err: DialError) -> RelayProbeError {
    match err {
        DialError::Dns { source, .. } => RelayProbeError::Dns(source.to_string()),
        DialError::Timeout { .. } => RelayProbeError::Timeout,
        DialError::Io { source, .. } => RelayProbeError::Other(format!("io: {source}")),
        DialError::InvalidUrl { url, .. } => {
            RelayProbeError::InvalidUrl(format!("invalid dial URL: {url}"))
        }
        DialError::InvalidTargetPort { .. } => {
            RelayProbeError::InvalidUrl("invalid target port".to_string())
        }
        DialError::ProxyConnectInvalidStatus { status, .. } => {
            RelayProbeError::Other(format!("proxy connect returned {status}"))
        }
        DialError::ProxyInvalidUrl { proxy_url, .. } => {
            RelayProbeError::Other(format!("invalid proxy URL: {proxy_url}"))
        }
        DialError::ProxyConnect { source, .. } => {
            RelayProbeError::Other(format!("proxy connect failed: {source}"))
        }
        DialError::ProxyInvalidTlsServername { proxy_hostname, .. } => {
            RelayProbeError::Tls(format!("invalid proxy TLS servername: {proxy_hostname}"))
        }
        DialError::ProxyInvalidTargetPort { .. } => {
            RelayProbeError::InvalidUrl("invalid proxy target port".to_string())
        }
        // 与 map_connect_error 同理:陌生 DialError 变体走 Other,源信息进
        // tracing 便于跨版本对账。
        other => {
            warn!(error = ?other, "relay probe: unmapped DialError variant");
            RelayProbeError::Other(other.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn rejects_empty_url() {
        let adapter = IrohRelayProbeAdapter::new().expect("init");
        let err = adapter.probe("   ").await.unwrap_err();
        assert!(matches!(err, RelayProbeError::InvalidUrl(_)));
    }

    #[tokio::test]
    async fn rejects_non_http_scheme() {
        let adapter = IrohRelayProbeAdapter::new().expect("init");
        let err = adapter.probe("ws://relay.example.com").await.unwrap_err();
        assert!(matches!(err, RelayProbeError::InvalidUrl(_)));
    }

    #[tokio::test]
    async fn rejects_garbage_string() {
        let adapter = IrohRelayProbeAdapter::new().expect("init");
        let err = adapter.probe("not a url").await.unwrap_err();
        assert!(matches!(err, RelayProbeError::InvalidUrl(_)));
    }

    /// 真握手回归用例。默认走 `--ignored` 跳过(CI / 离线开发不应依赖外部
    /// relay);本地排查 iroh-relay 升级 / 网络栈变更时:
    ///
    ///   RELAY_PROBE_TARGET=https://your-relay.example.com \
    ///     cargo test -p uc-infra relay_probe::tests \
    ///     probe_succeeds_against_real_relay -- --ignored --nocapture
    ///
    /// 不设环境变量则默认尝试 n0 公共 relay。若选定 relay 因 ISP / 区域
    /// 原因不可达,把 URL 换成离测试机更近的节点即可。
    #[tokio::test]
    #[ignore = "requires network access to a reachable iroh relay"]
    async fn probe_succeeds_against_real_relay() {
        let target = std::env::var("RELAY_PROBE_TARGET")
            .unwrap_or_else(|_| "https://use1-1.relay.iroh.network".to_string());
        let adapter = IrohRelayProbeAdapter::new().expect("init");
        let report = adapter
            .probe(&target)
            .await
            .unwrap_or_else(|err| panic!("probe failed against {target}: {err}"));
        assert!(
            report.latency_ms < 5_000,
            "probe latency {} ms exceeds budget",
            report.latency_ms
        );
    }
}
