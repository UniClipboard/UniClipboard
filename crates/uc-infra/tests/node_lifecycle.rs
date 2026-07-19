#![cfg(not(feature = "test-util"))]

// This test exercises the production single-node lease. Multi-node E2E
// harnesses enable `test-util` and intentionally use a different runtime shape.

use std::collections::HashMap;
use std::net::{Ipv4Addr, UdpSocket};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use uc_core::ports::{LocalIdentityPort, SecureStorageError, SecureStoragePort};
use uc_infra::network::iroh::{IrohIdentityStore, IrohNodeBuilder, IrohNodeConfig, IrohNodeError};
use uc_infra::security::Sha256IdentityFingerprintFactory;

#[derive(Default)]
struct InMemorySecureStorage {
    values: Mutex<HashMap<String, Vec<u8>>>,
}

impl SecureStoragePort for InMemorySecureStorage {
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>, SecureStorageError> {
        Ok(self.values.lock().unwrap().get(key).cloned())
    }

    fn set(&self, key: &str, value: &[u8]) -> Result<(), SecureStorageError> {
        self.values
            .lock()
            .unwrap()
            .insert(key.to_owned(), value.to_vec());
        Ok(())
    }

    fn delete(&self, key: &str) -> Result<(), SecureStorageError> {
        self.values.lock().unwrap().remove(key);
        Ok(())
    }
}

fn identity_store() -> IrohIdentityStore {
    IrohIdentityStore::new(
        Arc::new(InMemorySecureStorage::default()),
        Arc::new(Sha256IdentityFingerprintFactory),
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn production_node_restarts_ten_times_with_stable_identity_and_released_port() {
    let port_probe = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).expect("reserve test port");
    let port = port_probe.local_addr().expect("read test port").port();
    drop(port_probe);

    let identity_store = identity_store();
    let config = IrohNodeConfig {
        bind_port: Some(port),
        disable_relays: true,
        ..Default::default()
    };
    let mut expected_identity = None;

    for cycle in 0..10 {
        let builder = tokio::time::timeout(
            Duration::from_secs(30),
            IrohNodeBuilder::bind(&identity_store, config.clone()),
        )
        .await
        .expect("production bind exceeded 30 seconds")
        .expect("bind production node");

        if cycle == 0 {
            let duplicate = tokio::time::timeout(
                Duration::from_secs(5),
                IrohNodeBuilder::bind(&identity_store, config.clone()),
            )
            .await
            .expect("concurrent production bind exceeded 5 seconds");
            assert!(matches!(duplicate, Err(IrohNodeError::AlreadyRunning)));
        }

        let identity = identity_store
            .get_current_fingerprint()
            .await
            .expect("read persistent identity")
            .expect("bind creates identity");
        if let Some(expected) = &expected_identity {
            assert_eq!(&identity, expected, "identity changed on restart {cycle}");
        } else {
            expected_identity = Some(identity);
        }

        tokio::time::timeout(Duration::from_secs(30), builder.spawn().shutdown())
            .await
            .expect("production shutdown exceeded 30 seconds");

        let released = UdpSocket::bind((Ipv4Addr::LOCALHOST, port))
            .expect("shutdown must release the fixed UDP port");
        drop(released);
    }
}
