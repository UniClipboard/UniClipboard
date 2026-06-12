use std::collections::HashMap;
use std::sync::Mutex;

/// 出站 transfer_id 到 entry_id 的轻量提示缓存。
#[derive(Default)]
pub struct OutboundEntryIdCache {
    inner: Mutex<HashMap<String, String>>,
}

impl OutboundEntryIdCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&self, transfer_id: impl Into<String>, entry_id: impl Into<String>) {
        let mut map = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        map.insert(transfer_id.into(), entry_id.into());
    }

    pub fn get(&self, transfer_id: &str) -> Option<String> {
        let map = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        map.get(transfer_id).cloned()
    }

    pub fn remove(&self, transfer_id: &str) {
        let mut map = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        map.remove(transfer_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_then_get_returns_entry_id() {
        let cache = OutboundEntryIdCache::new();
        cache.insert("tx-1", "entry-1");
        assert_eq!(cache.get("tx-1"), Some("entry-1".to_string()));
    }

    #[test]
    fn get_missing_returns_none() {
        let cache = OutboundEntryIdCache::new();
        assert!(cache.get("tx-missing").is_none());
    }

    #[test]
    fn remove_drops_entry() {
        let cache = OutboundEntryIdCache::new();
        cache.insert("tx-1", "entry-1");
        cache.remove("tx-1");
        assert!(cache.get("tx-1").is_none());
    }
}
