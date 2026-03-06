use crate::ClipboardChangeOrigin;
use async_trait::async_trait;
use std::time::Duration;

#[async_trait]
pub trait ClipboardChangeOriginPort: Send + Sync {
    async fn set_next_origin(&self, origin: ClipboardChangeOrigin, ttl: Duration);

    async fn consume_origin_or_default(
        &self,
        default_origin: ClipboardChangeOrigin,
    ) -> ClipboardChangeOrigin;

    async fn remember_remote_snapshot_hash(&self, _snapshot_hash: String, _ttl: Duration) {}

    async fn consume_origin_for_snapshot_or_default(
        &self,
        _snapshot_hash: &str,
        default_origin: ClipboardChangeOrigin,
    ) -> ClipboardChangeOrigin {
        self.consume_origin_or_default(default_origin).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockOriginPort;

    #[async_trait::async_trait]
    impl ClipboardChangeOriginPort for MockOriginPort {
        async fn set_next_origin(&self, _origin: ClipboardChangeOrigin, _ttl: std::time::Duration) {
        }

        async fn consume_origin_or_default(
            &self,
            default_origin: ClipboardChangeOrigin,
        ) -> ClipboardChangeOrigin {
            default_origin
        }
    }

    #[tokio::test]
    async fn origin_port_returns_default() {
        let port = MockOriginPort;
        let origin = port
            .consume_origin_or_default(ClipboardChangeOrigin::LocalCapture)
            .await;
        assert_eq!(origin, ClipboardChangeOrigin::LocalCapture);
    }
}
