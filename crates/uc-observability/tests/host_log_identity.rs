//! Identity fields of host JSON log records: a per-process run identity on every
//! record (D02) and no raw device, peer or session-token identifiers (D09).

use std::io::Write;
use std::sync::{Arc, Mutex};

use tracing_subscriber::fmt::MakeWriter;
use uc_observability::format::FlatJsonFormat;

#[derive(Clone, Default)]
struct SharedBuffer(Arc<Mutex<Vec<u8>>>);

impl Write for SharedBuffer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("buffer").extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for SharedBuffer {
    type Writer = SharedBuffer;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

fn capture(emit: impl FnOnce()) -> Vec<serde_json::Value> {
    let buffer = SharedBuffer::default();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(buffer.clone())
        .event_format(FlatJsonFormat::new())
        .finish();
    tracing::subscriber::with_default(subscriber, emit);
    let bytes = buffer.0.lock().expect("buffer").clone();
    String::from_utf8(bytes)
        .expect("utf8")
        .lines()
        .map(|line| serde_json::from_str(line).expect("json line"))
        .collect()
}

#[test]
fn every_record_carries_the_run_id_and_pid() {
    let records = capture(|| {
        tracing::info!("first");
        let span = tracing::info_span!("work", step = 1);
        let _entered = span.enter();
        tracing::warn!("second");
    });

    assert_eq!(records.len(), 2);
    let run_id = records[0]["run_id"].as_str().expect("run_id is a string");
    uuid::Uuid::parse_str(run_id).expect("run_id is a UUID");
    for record in &records {
        assert_eq!(record["run_id"], run_id, "one run id per process");
        assert_eq!(record["pid"], u64::from(std::process::id()));
    }
}

#[test]
fn an_event_field_cannot_override_the_run_identity() {
    let records = capture(|| tracing::info!(run_id = "spoofed", pid = 1_u64, "event"));

    let record = &records[0];
    assert_ne!(record["run_id"], "spoofed");
    assert_eq!(record["pid"], u64::from(std::process::id()));
    assert_eq!(record["event_run_id"], "spoofed");
    assert_eq!(record["event_pid"], 1);
}

const DEVICE_ID: &str = "0b9c3a52-7d1e-4f60-9a84-5c2e1d7f3b01";
const PEER_ID: &str = "k51qzi5uqu5dh9ihj3wz2z8k7yv8m0x4r6t1a2b3c4d5e6f7g8h9i0j1k2l3m";
const TOKEN_JTI: &str = "7f3d2c1b-aaaa-bbbb-cccc-0123456789ab";

#[test]
fn raw_identifiers_never_reach_the_host_log() {
    let _ = uc_observability::set_global_device_id(DEVICE_ID.to_string());
    let records = capture(|| {
        let span = tracing::info_span!("session", session_token_jti = TOKEN_JTI);
        let _entered = span.enter();
        tracing::info!(device_id = DEVICE_ID, peer_id = PEER_ID, "connected");
        tracing::info!("no explicit identifiers");
    });

    for record in &records {
        let line = record.to_string();
        for raw in [DEVICE_ID, PEER_ID, TOKEN_JTI] {
            assert!(!line.contains(raw), "raw identifier leaked: {line}");
        }
    }
}

#[test]
fn refs_correlate_within_a_run_and_distinguish_values() {
    let records = capture(|| {
        tracing::info!(peer_id = PEER_ID, "a");
        tracing::info!(peer_id = PEER_ID, "b");
        tracing::info!(peer_id = "another-peer", "c");
    });

    let first = records[0]["peer_ref"].as_str().expect("peer_ref");
    assert_eq!(records[1]["peer_ref"], first);
    assert_ne!(records[2]["peer_ref"], first);
    assert!(
        records[0].get("peer_id").is_none(),
        "the raw key is dropped"
    );
}

#[test]
fn the_process_device_is_referenced_not_named() {
    let _ = uc_observability::set_global_device_id(DEVICE_ID.to_string());
    let records = capture(|| tracing::info!("hello"));

    let record = &records[0];
    assert!(record.get("device_id").is_none());
    assert!(record["device_ref"].as_str().is_some_and(|r| !r.is_empty()));
}
