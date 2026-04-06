use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use serial_test::serial;
use tracing_subscriber::prelude::*;

use super::config::{
    prime_env_pair_from_baked, OTEL_ENDPOINT_VAR, OTEL_HEADERS_VAR, OTEL_TRACES_ENDPOINT_VAR,
    OTEL_TRACES_HEADERS_VAR,
};
use super::{init_otlp_pipeline, init_otlp_provider};
use crate::LogProfile;

struct EnvVarGuard {
    key: &'static str,
    original: Option<String>,
}

impl EnvVarGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let original = std::env::var(key).ok();
        std::env::set_var(key, value);
        Self { key, original }
    }

    fn unset(key: &'static str) -> Self {
        let original = std::env::var(key).ok();
        std::env::remove_var(key);
        Self { key, original }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        if let Some(value) = self.original.as_ref() {
            std::env::set_var(self.key, value);
        } else {
            std::env::remove_var(self.key);
        }
    }
}

fn spawn_http_probe() -> (String, mpsc::Receiver<String>, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind OTLP probe");
    let address = listener.local_addr().expect("read OTLP probe address");
    let (tx, rx) = mpsc::channel();
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept OTLP request");
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("set probe timeout");

        let mut buffer = Vec::new();
        let mut chunk = [0_u8; 4096];
        let mut header_end = None;
        let mut content_length = 0_usize;

        loop {
            let read = stream.read(&mut chunk).expect("read OTLP request");
            if read == 0 {
                break;
            }

            buffer.extend_from_slice(&chunk[..read]);

            if header_end.is_none() {
                header_end = buffer.windows(4).position(|window| window == b"\r\n\r\n");
                if let Some(index) = header_end {
                    let header_bytes = &buffer[..index + 4];
                    let headers = String::from_utf8_lossy(header_bytes);
                    for line in headers.lines() {
                        if let Some(value) = line.strip_prefix("Content-Length:") {
                            content_length = value.trim().parse().expect("parse content length");
                        }
                    }
                }
            }

            if let Some(index) = header_end {
                let expected_len = index + 4 + content_length;
                if buffer.len() >= expected_len {
                    break;
                }
            }
        }

        let request = String::from_utf8_lossy(&buffer);
        let first_line = request.lines().next().unwrap_or_default().to_string();
        tx.send(first_line).expect("send request line");

        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
            .expect("write probe response");
    });

    (format!("http://{}", address), rx, handle)
}

#[test]
#[serial]
fn baked_endpoint_only_applies_when_runtime_endpoint_is_missing() {
    const TEST_GENERIC: &str = "UC_TEST_OTLP_GENERIC";
    const TEST_SPECIFIC: &str = "UC_TEST_OTLP_SPECIFIC";

    let _generic = EnvVarGuard::unset(TEST_GENERIC);
    let _specific = EnvVarGuard::unset(TEST_SPECIFIC);

    prime_env_pair_from_baked(TEST_SPECIFIC, TEST_GENERIC, None, Some("http://baked"));

    assert_eq!(
        std::env::var(TEST_GENERIC).ok().as_deref(),
        Some("http://baked")
    );
}

#[test]
#[serial]
fn baked_endpoint_does_not_override_runtime_endpoint() {
    const TEST_GENERIC: &str = "UC_TEST_OTLP_GENERIC";
    const TEST_SPECIFIC: &str = "UC_TEST_OTLP_SPECIFIC";

    let _generic = EnvVarGuard::set(TEST_GENERIC, "http://runtime");
    let _specific = EnvVarGuard::unset(TEST_SPECIFIC);

    prime_env_pair_from_baked(TEST_SPECIFIC, TEST_GENERIC, None, Some("http://baked"));

    assert_eq!(
        std::env::var(TEST_GENERIC).ok().as_deref(),
        Some("http://runtime")
    );
}

#[test]
#[serial]
fn prod_profile_disables_otlp_when_telemetry_disabled() {
    let _generic = EnvVarGuard::set(OTEL_ENDPOINT_VAR, "http://127.0.0.1:4318");
    let _signal = EnvVarGuard::unset(OTEL_TRACES_ENDPOINT_VAR);

    let provider = init_otlp_provider(&LogProfile::Prod, None, false).expect("init_otlp_provider");

    assert!(
        provider.is_none(),
        "Prod profile with telemetry_enabled=false should keep OTLP disabled"
    );
}

#[test]
#[serial]
fn generic_runtime_endpoint_uses_standard_traces_path() {
    let (base_url, rx, handle) = spawn_http_probe();
    let _generic = EnvVarGuard::set(OTEL_ENDPOINT_VAR, &format!("{base_url}/ingest/otlp"));
    let _signal = EnvVarGuard::unset(OTEL_TRACES_ENDPOINT_VAR);
    let _headers = EnvVarGuard::unset(OTEL_HEADERS_VAR);
    let _signal_headers = EnvVarGuard::unset(OTEL_TRACES_HEADERS_VAR);

    let (layer, guard) = init_otlp_pipeline(&LogProfile::Dev, None, true)
        .expect("init_otlp_pipeline")
        .expect("OTLP layer enabled");
    let subscriber = tracing_subscriber::registry().with(layer);

    tracing::subscriber::with_default(subscriber, || {
        tracing::info_span!("otlp.test").in_scope(|| {});
    });

    drop(guard);

    let request_line = rx
        .recv_timeout(Duration::from_secs(5))
        .expect("receive OTLP request line");
    handle.join().expect("join OTLP probe");

    assert_eq!(request_line, "POST /ingest/otlp/v1/traces HTTP/1.1");
}
