//! Slice 3 Phase 1 T0 — 探针锁定 iroh-blobs 与当前 iroh endpoint 的接口契约。
//!
//! 这个测试不验证 UniClipboard 业务逻辑,只回答 adapter 落地前必须确定的
//! 第三方 API 问题:
//!
//! 1. `FsStore::load` + `blobs().add_bytes` + `get_bytes` 是否能稳定完成本地回环。
//! 2. `tags().set/get/delete` 的签名和幂等语义是否符合 `tag/untag` 设计。
//! 3. `BlobTicket` 是否能用二进制 ticket trait 稳定 round-trip。
//! 4. `BlobsProtocol` 是否能挂到当前共享 `iroh 0.95` router,并通过 downloader 拉取。

use std::{path::PathBuf, time::Duration};

use iroh::{
    address_lookup::memory::MemoryLookup, protocol::Router, Endpoint, EndpointAddr, RelayMode,
};
use iroh_blobs::{store::fs::FsStore, ticket::BlobTicket, BlobFormat, BlobsProtocol, Hash};
use iroh_tickets::Ticket;

struct BlobNode {
    router: Router,
    store: FsStore,
    _path: PathBuf,
    discovery: MemoryLookup,
}

impl BlobNode {
    async fn bind(path: PathBuf) -> anyhow::Result<Self> {
        let store = FsStore::load(&path).await?;
        let discovery = MemoryLookup::new();
        let endpoint = Endpoint::builder(iroh::endpoint::presets::N0)
            .relay_mode(RelayMode::Disabled)
            .address_lookup(discovery.clone())
            .bind()
            .await?;
        let protocol = BlobsProtocol::new(&store, None);
        let router = Router::builder(endpoint)
            .accept(iroh_blobs::ALPN, protocol)
            .spawn();

        Ok(Self {
            router,
            store,
            _path: path,
            discovery,
        })
    }

    fn addr(&self) -> EndpointAddr {
        self.router.endpoint().addr()
    }

    async fn wait_for_direct_addr(&self) -> anyhow::Result<()> {
        for _ in 0..100 {
            if self.addr().ip_addrs().next().is_some() {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        anyhow::bail!("iroh endpoint never published a loopback direct address")
    }

    async fn shutdown(self) -> anyhow::Result<()> {
        self.router.shutdown().await?;
        Ok(())
    }
}

#[tokio::test]
async fn fs_store_add_get_observe_round_trips_bytes() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let store = FsStore::load(temp.path().join("store")).await?;
    let payload = b"slice3-t0-local-store".to_vec();

    let tag = store.blobs().add_bytes(payload.clone()).await?;
    assert_eq!(tag.hash, Hash::new(&payload));

    let observed = store.blobs().observe(tag.hash).await_completion().await?;
    assert!(
        observed.is_complete(),
        "add_bytes 后 observe 应该显示本地 blob 已完整"
    );

    let round_tripped = store.blobs().get_bytes(tag.hash).await?;
    assert_eq!(round_tripped.as_ref(), payload.as_slice());

    store.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn tags_set_get_delete_are_stable_and_delete_is_idempotent() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let store = FsStore::load(temp.path().join("store")).await?;
    let tag_name = b"uc:slice3:t0:tag";

    let tag = store
        .blobs()
        .add_bytes(b"tagged-payload".as_slice())
        .await?;
    store.tags().set(tag_name, tag.hash_and_format()).await?;

    let loaded = store
        .tags()
        .get(tag_name)
        .await?
        .expect("tag should exist after set");
    assert_eq!(loaded.hash_and_format(), tag.hash_and_format());

    assert_eq!(store.tags().delete(tag_name).await?, 1);
    assert_eq!(store.tags().delete(tag_name).await?, 0);
    assert!(
        store.tags().get(tag_name).await?.is_none(),
        "delete 后 tag 查询应为空"
    );

    store.shutdown().await?;
    Ok(())
}

#[test]
fn blob_ticket_bytes_round_trip_preserves_addr_hash_and_format() -> anyhow::Result<()> {
    let endpoint_id = iroh::SecretKey::from_bytes(&[7u8; 32]).public();
    let addr = EndpointAddr::new(endpoint_id);
    let hash = Hash::new(b"ticket-payload");
    let ticket = BlobTicket::new(addr.clone(), hash, BlobFormat::Raw);

    let bytes = ticket.to_bytes();
    let decoded = BlobTicket::from_bytes(&bytes)?;

    assert_eq!(decoded.addr(), &addr);
    assert_eq!(decoded.hash(), hash);
    assert_eq!(decoded.format(), BlobFormat::Raw);
    assert_eq!(decoded.to_bytes(), bytes);

    Ok(())
}

#[tokio::test]
async fn blobs_protocol_router_and_downloader_fetch_between_loopback_nodes() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let provider = BlobNode::bind(temp.path().join("provider")).await?;
    let receiver = BlobNode::bind(temp.path().join("receiver")).await?;
    provider.wait_for_direct_addr().await?;
    receiver.wait_for_direct_addr().await?;

    provider.discovery.add_endpoint_info(receiver.addr());
    receiver.discovery.add_endpoint_info(provider.addr());

    let payload = b"slice3-t0-downloader-loopback".to_vec();
    let provider_tag = provider.store.blobs().add_bytes(payload.clone()).await?;
    let ticket = BlobTicket::new(provider.addr(), provider_tag.hash, provider_tag.format);

    let connection = receiver
        .router
        .endpoint()
        .connect(ticket.addr().clone(), iroh_blobs::ALPN)
        .await?;
    drop(connection);

    receiver
        .store
        .downloader(receiver.router.endpoint())
        .download(ticket.hash_and_format(), [ticket.addr().id])
        .await?;

    let received = receiver.store.blobs().get_bytes(ticket.hash()).await?;
    assert_eq!(received.as_ref(), payload.as_slice());

    receiver.shutdown().await?;
    provider.shutdown().await?;
    Ok(())
}
