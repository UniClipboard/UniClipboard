//! 进程内会话存储——`SpaceAccessAdapter` / `BlobCipherAdapter` /
//! `TransferCipherAdapter` / `EncryptedBlobStore` 共享同一份 `Arc<InMemorySession>`。
//!
//! 历史上这是 `InMemoryEncryptionSessionPort`（uc-platform 的 trait 实现);
//! Slice 3 - C8 把 `EncryptionSessionPort` trait 删除后,这个类型下沉到
//! uc-infra 作为具体类型——所有 uc-infra 内部 adapter 共用同一个 Arc,
//! 不再走 dyn trait 间接层。

use std::sync::{Arc, Mutex};

use tracing::{debug, debug_span};
use uc_core::crypto::model::EncryptionError;

use super::secrets::MasterKey;

#[derive(Debug)]
struct State {
    master_key: Option<MasterKey>,
}

/// In-memory master-key 容器,线程安全。
///
/// `MasterKey` 派生 `ZeroizeOnDrop`(见 `super::secrets`),所以
/// `set_master_key` 替换旧值、`clear` 把 `Option` 置空、整个 `InMemorySession`
/// 被 drop 等路径都会就地把 32 字节密钥清零——会话生命周期结束后,残留密钥
/// 物料就不会停留在堆/栈/swap 页面里。
#[derive(Clone)]
pub struct InMemorySession {
    state: Arc<Mutex<State>>,
}

impl InMemorySession {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(State { master_key: None })),
        }
    }

    pub fn is_ready(&self) -> bool {
        self.state
            .lock()
            .expect("session lock")
            .master_key
            .is_some()
    }

    pub fn get_master_key(&self) -> Result<MasterKey, EncryptionError> {
        self.state
            .lock()
            .expect("session lock")
            .master_key
            .as_ref()
            .cloned()
            .ok_or(EncryptionError::NotInitialized)
    }

    pub fn set_master_key(&self, master_key: MasterKey) {
        let span = debug_span!("infra.session.set_master_key");
        span.in_scope(|| {
            // 替换旧密钥——旧 MasterKey 被 drop 时由 ZeroizeOnDrop 清零。
            self.state.lock().expect("session lock").master_key = Some(master_key);
            debug!("master key set");
        });
    }

    pub fn clear(&self) {
        let span = debug_span!("infra.session.clear");
        span.in_scope(|| {
            self.state.lock().expect("session lock").master_key = None;
            debug!("master key cleared");
        });
    }
}

impl Default for InMemorySession {
    fn default() -> Self {
        Self::new()
    }
}
