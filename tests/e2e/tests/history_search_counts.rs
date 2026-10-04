//! E2E: History search candidate counts, zero-result relaxations and the
//! content-lock boundary of `POST /search/count`, against a real daemon.
//!
//! Isolation: a fresh `e2e-history-search-*` profile, daemon in server mode
//! (system clipboard replaced by a no-op), no rendezvous, no GUI. History is
//! seeded with the development CLI while the daemon is stopped.
//!
//! The count queries below are the exact wire params the History page builds
//! (`buildCandidateCountQueries` / `buildRelaxationQueries` in
//! `apps/gui/src/components/history/composite-search/composite-search-model.ts`).
//!
//! Optional browser step: set `UC_E2E_HISTORY_BROWSER=1` to drive the real
//! search components in headless Chromium against this daemon
//! (`apps/gui/e2e/history-search-real-daemon-browser.mjs`).
//!
//! Artifacts (inputs, every request/response, daemon log, browser output) go
//! to `UC_E2E_ARTIFACT_DIR`, default `target/e2e-artifacts/history-search`.
//!
//! Run (needs `cargo build -p uc-daemon -p uc-cli --features uc-cli/dev-tools`):
//!   cargo test --manifest-path tests/e2e/Cargo.toml --test history_search_counts -- --ignored --nocapture

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use uc_e2e_tests::{read_daemon_file_token, NodeBinarySet, TestCli, TestDaemon, TestProfile};

const PASSPHRASE: &str = "history-search-e2e-passphrase";

const TEXTS: &[&str] = &[
    "release notes for version 1.2",
    "meeting agenda: roadmap review",
    "grocery list: milk, eggs, coffee",
];
const FILES: &[(&str, &str)] = &[
    ("design-notes.md", "# Design notes\n"),
    ("todo.txt", "todo\n"),
];

/// Records every request and, on drop (pass or fail), writes them plus the
/// daemon logs. Create it after the daemon so it drops before the profile
/// directories are cleaned up.
struct Recorder {
    dir: PathBuf,
    log: Vec<Value>,
    daemon_logs: Vec<PathBuf>,
}

impl Recorder {
    fn new(profile: &TestProfile) -> Self {
        let dir = std::env::var_os("UC_E2E_ARTIFACT_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../target/e2e-artifacts/history-search")
            });
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("artifact dir");
        Self {
            dir,
            log: Vec::new(),
            daemon_logs: vec![profile.process_log_path(), profile.log_dir().clone()],
        }
    }

    fn write(&self, name: &str, value: &Value) {
        std::fs::write(
            self.dir.join(name),
            serde_json::to_string_pretty(value).unwrap(),
        )
        .expect("write artifact");
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        self.write("requests.json", &Value::Array(self.log.clone()));
        for source in &self.daemon_logs {
            if source.is_dir() {
                for entry in std::fs::read_dir(source).into_iter().flatten().flatten() {
                    let _ = std::fs::copy(entry.path(), self.dir.join(entry.file_name()));
                }
            } else if let Some(name) = source.file_name() {
                let _ = std::fs::copy(source, self.dir.join(name));
            }
        }
    }
}

struct Api {
    base: String,
    client: reqwest::Client,
}

impl Api {
    async fn connect(daemon: &TestDaemon, client_type: &str) -> (Self, String) {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .unwrap();
        let file_token = read_daemon_file_token(daemon);
        let resp = client
            .post(format!("{}/auth/connect", daemon.base_url()))
            .header("Authorization", format!("Bearer {file_token}"))
            .json(&json!({ "pid": std::process::id(), "clientType": client_type }))
            .send()
            .await
            .expect("auth/connect");
        assert!(
            resp.status().is_success(),
            "auth/connect {client_type}: {}",
            resp.status()
        );
        let body: Value = resp.json().await.unwrap();
        let token = body["data"]["sessionToken"].as_str().unwrap().to_string();
        (
            Self {
                base: daemon.base_url(),
                client,
            },
            token,
        )
    }

    async fn call(
        &self,
        rec: &mut Recorder,
        label: &str,
        token: &str,
        method: reqwest::Method,
        path: &str,
        query: Option<&Value>,
        body: Option<&Value>,
    ) -> (u16, Value) {
        let mut req = self
            .client
            .request(method.clone(), format!("{}{}", self.base, path))
            .header("Authorization", format!("Session {token}"));
        if let Some(q) = query {
            let pairs: Vec<(String, String)> = q
                .as_object()
                .unwrap()
                .iter()
                .map(|(k, v)| {
                    (
                        k.clone(),
                        v.as_str()
                            .map(str::to_string)
                            .unwrap_or_else(|| v.to_string()),
                    )
                })
                .collect();
            req = req.query(&pairs);
        }
        if let Some(b) = body {
            req = req.json(b);
        }
        let resp = req.send().await.expect("request");
        let status = resp.status().as_u16();
        let value: Value = resp.json().await.unwrap_or(Value::Null);
        rec.log.push(json!({
            "label": label,
            "method": method.as_str(),
            "path": path,
            "query": query,
            "body": body,
            "status": status,
            "response": value,
        }));
        (status, value)
    }
}

fn cli_ok(cli: &TestCli, args: &[&str]) -> String {
    let out = cli.run_capture(args);
    assert!(
        out.success(),
        "uniclip {args:?} failed: {} {}",
        out.stdout,
        out.stderr
    );
    out.stdout
}

/// `GET /search/query` takes the same filter keys as one `/search/count` item.
fn as_query(params: &Value) -> Value {
    let mut q = params.clone();
    q["limit"] = json!("200");
    q
}

/// Seeding writes the main store directly, bypassing the live index, so run
/// the production full rebuild and wait until one completes after `since_ms`.
async fn rebuild_index(api: &Api, rec: &mut Recorder, token: &str) {
    let since_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        let (status, body) = api
            .call(
                rec,
                "search-rebuild",
                token,
                reqwest::Method::POST,
                "/search/rebuild",
                None,
                None,
            )
            .await;
        if status == 202 {
            break;
        }
        // 409: an automatic rebuild is already running; 503: index not up yet.
        assert!(matches!(status, 409 | 503), "rebuild: {status} {body}");
        assert!(Instant::now() < deadline, "rebuild never accepted: {body}");
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    loop {
        let (status, body) = api
            .call(
                rec,
                "search-status",
                token,
                reqwest::Method::GET,
                "/search/status",
                None,
                None,
            )
            .await;
        let data = &body["data"];
        if status == 200
            && data["state"] == "ready"
            && data["lastRebuildCompletedAtMs"]
                .as_i64()
                .is_some_and(|t| t >= since_ms)
        {
            return;
        }
        assert!(Instant::now() < deadline, "rebuild never completed: {body}");
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

async fn counts(
    api: &Api,
    rec: &mut Recorder,
    label: &str,
    token: &str,
    queries: &[Value],
) -> Vec<u64> {
    let (status, body) = api
        .call(
            rec,
            label,
            token,
            reqwest::Method::POST,
            "/search/count",
            None,
            Some(&json!({ "queries": queries })),
        )
        .await;
    assert_eq!(status, 200, "{label}: {body}");
    body["data"]["counts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c.as_u64().unwrap())
        .collect()
}

async fn total(api: &Api, rec: &mut Recorder, label: &str, token: &str, params: &Value) -> u64 {
    let (status, body) = api
        .call(
            rec,
            label,
            token,
            reqwest::Method::GET,
            "/search/query",
            Some(&as_query(params)),
            None,
        )
        .await;
    assert_eq!(status, 200, "{label}: {body}");
    body["data"]["total"].as_u64().unwrap()
}

#[tokio::test]
#[ignore]
async fn history_search_counts_relaxations_and_content_lock() {
    let binaries = NodeBinarySet::current_dev_cli();
    let profile = TestProfile::new("history-search");
    let mut daemon = TestDaemon::start_clean_with(profile, &binaries, None)
        .await
        .expect("daemon start");
    let mut rec = Recorder::new(&daemon.profile);
    let cli = TestCli::with_binaries(&daemon.profile, &binaries);
    cli_ok(
        &cli,
        &[
            "init",
            "--passphrase",
            PASSPHRASE,
            "--device-name",
            "search-node",
        ],
    );

    // ── Seed history with the daemon stopped (dev commands refuse otherwise).
    daemon.kill();
    let files_dir = daemon.profile.data_dir().join("e2e-seed-files");
    std::fs::create_dir_all(&files_dir).unwrap();
    for text in TEXTS {
        cli_ok(&cli, &["dev", "seed-clipboard", "--text", text]);
    }
    for (name, content) in FILES {
        let path = files_dir.join(name);
        std::fs::write(&path, content).unwrap();
        cli_ok(
            &cli,
            &["dev", "capture-files", "--path", path.to_str().unwrap()],
        );
    }
    rec.write(
        "inputs.json",
        &json!({
            "profile": daemon.profile.name,
            "texts": TEXTS,
            "files": FILES.iter().map(|(n, c)| json!({ "name": n, "content": c })).collect::<Vec<_>>(),
        }),
    );
    daemon
        .restart_preserving_configured_with(|_| {})
        .await
        .expect("daemon restart");

    let (api, cli_token) = Api::connect(&daemon, "cli").await;
    rebuild_index(&api, &mut rec, &cli_token).await;
    let all = total(
        &api,
        &mut rec,
        "query-all",
        &cli_token,
        &json!({ "query": "" }),
    )
    .await;
    assert_eq!(
        all,
        (TEXTS.len() + FILES.len()) as u64,
        "every seeded entry is indexed"
    );

    // ── B2: `type:` candidates with `time:` held at all_time (no timePreset).
    // Order matches TYPE_FILTERS: text, rich text, image (a tag), file.
    let type_candidates = [
        json!({ "query": "", "contentTypes": "text" }),
        json!({ "query": "", "contentTypes": "html" }),
        json!({ "query": "", "tags": "image" }),
        json!({ "query": "", "contentTypes": "file" }),
    ];
    let type_counts = counts(
        &api,
        &mut rec,
        "count-type-candidates",
        &cli_token,
        &type_candidates,
    )
    .await;
    for (params, count) in type_candidates.iter().zip(&type_counts) {
        let listed = total(&api, &mut rec, "query-type-candidate", &cli_token, params).await;
        assert_eq!(
            *count, listed,
            "count must equal the list total for {params}"
        );
    }
    // `dev seed-clipboard` skips normalization, so seeded text/plain entries
    // index as `other`, not `text`; only captured files carry a type the
    // History type filters target.
    assert_eq!(
        type_counts,
        vec![0, 0, 0, FILES.len() as u64],
        "type candidates: text, rich text, image, file"
    );

    // Same candidates with another filter held fixed (`ext:md`).
    let with_ext: Vec<Value> = type_candidates
        .iter()
        .map(|p| {
            let mut p = p.clone();
            p["extensions"] = json!("md");
            p
        })
        .collect();
    let ext_counts = counts(
        &api,
        &mut rec,
        "count-type-candidates-ext-md",
        &cli_token,
        &with_ext,
    )
    .await;
    assert_eq!(ext_counts[0], 0, "no text entry has a .md extension");
    assert_eq!(ext_counts[3], 1, "exactly one .md file");

    // ── B3: `agenda` + `type:file` finds nothing; drop one chip at a time.
    let zero = json!({ "query": "agenda", "contentTypes": "file" });
    assert_eq!(
        total(&api, &mut rec, "query-zero-result", &cli_token, &zero).await,
        0
    );
    let relaxations = [json!({ "query": "agenda" })]; // only chip: type
    let relax_counts = counts(
        &api,
        &mut rec,
        "count-relaxations",
        &cli_token,
        &relaxations,
    )
    .await;
    assert_eq!(
        relax_counts,
        vec![1],
        "dropping type:file leaves the agenda text"
    );

    let zero_two = json!({ "query": "agenda", "contentTypes": "file", "extensions": "md" });
    assert_eq!(
        total(&api, &mut rec, "query-zero-result-2", &cli_token, &zero_two).await,
        0
    );
    let relax_two = [
        json!({ "query": "agenda", "extensions": "md" }), // drop type
        json!({ "query": "agenda", "contentTypes": "file" }), // drop ext
    ];
    let relax_two_counts = counts(
        &api,
        &mut rec,
        "count-relaxations-2",
        &cli_token,
        &relax_two,
    )
    .await;
    assert_eq!(
        relax_two_counts,
        vec![0, 0],
        "neither single drop helps: both shown disabled"
    );

    // Batch cap.
    let too_many: Vec<Value> = (0..33).map(|_| json!({ "query": "" })).collect();
    let (status, body) = api
        .call(
            &mut rec,
            "count-over-batch-cap",
            &cli_token,
            reqwest::Method::POST,
            "/search/count",
            None,
            Some(&json!({ "queries": too_many })),
        )
        .await;
    assert_eq!(status, 400, "33 queries must be rejected: {body}");

    // ── Optional browser step against this daemon (GUI client session).
    if std::env::var("UC_E2E_HISTORY_BROWSER").as_deref() == Ok("1") {
        let (_, gui_token) = Api::connect(&daemon, "gui").await;
        let (status, body) = api
            .call(
                &mut rec,
                "gui-unlock-content",
                &gui_token,
                reqwest::Method::POST,
                "/content-lock/unlock",
                None,
                Some(&json!({ "passphrase": PASSPHRASE })),
            )
            .await;
        assert_eq!(status, 200, "gui content unlock: {body}");
        let gui_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/gui");
        let out = Command::new("node")
            .arg("e2e/history-search-real-daemon-browser.mjs")
            .current_dir(&gui_dir)
            .env("UC_E2E_DAEMON_URL", daemon.base_url())
            .env("UC_E2E_GUI_TOKEN", &gui_token)
            .env("UC_E2E_ARTIFACT_DIR", rec.dir.join("browser"))
            .output()
            .expect("run browser step");
        std::fs::write(
            rec.dir.join("browser-step.log"),
            [out.stdout.as_slice(), out.stderr.as_slice()].concat(),
        )
        .unwrap();
        assert!(
            out.status.success(),
            "browser step failed:\n{}\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }

    // ── Content-lock boundary: gated GUI session vs CLI session.
    let (_, gui_token) = Api::connect(&daemon, "gui").await;
    let one = [json!({ "query": "" })];
    let (status, _) = api
        .call(
            &mut rec,
            "gui-revoke-content",
            &gui_token,
            reqwest::Method::POST,
            "/content-lock/revoke",
            None,
            None,
        )
        .await;
    assert_eq!(status, 200);
    let (status, body) = api
        .call(
            &mut rec,
            "gui-count-while-content-locked",
            &gui_token,
            reqwest::Method::POST,
            "/search/count",
            None,
            Some(&json!({ "queries": one })),
        )
        .await;
    assert_eq!(status, 423, "gui count while content locked: {body}");
    assert_eq!(body["code"], "content_locked", "{body}");
    let (status, _) = api
        .call(
            &mut rec,
            "gui-status-while-content-locked",
            &gui_token,
            reqwest::Method::GET,
            "/search/status",
            None,
            None,
        )
        .await;
    assert_eq!(
        status, 200,
        "index status stays open while content is locked"
    );
    assert_eq!(
        counts(
            &api,
            &mut rec,
            "cli-count-while-content-locked",
            &cli_token,
            &one
        )
        .await,
        vec![(TEXTS.len() + FILES.len()) as u64],
        "the CLI is not a gated client"
    );
    let (status, body) = api
        .call(
            &mut rec,
            "gui-unlock-content",
            &gui_token,
            reqwest::Method::POST,
            "/content-lock/unlock",
            None,
            Some(&json!({ "passphrase": PASSPHRASE })),
        )
        .await;
    assert_eq!(status, 200, "content unlock: {body}");
    assert_eq!(
        counts(&api, &mut rec, "gui-count-after-unlock", &gui_token, &one).await,
        vec![(TEXTS.len() + FILES.len()) as u64]
    );

    // ── Encryption session locked: every client gets session_locked.
    let (status, body) = api
        .call(
            &mut rec,
            "lock-encryption",
            &cli_token,
            reqwest::Method::POST,
            "/encryption/lock",
            None,
            None,
        )
        .await;
    assert!(
        (200..300).contains(&status),
        "encryption lock: {status} {body}"
    );
    let (status, body) = api
        .call(
            &mut rec,
            "cli-count-while-session-locked",
            &cli_token,
            reqwest::Method::POST,
            "/search/count",
            None,
            Some(&json!({ "queries": one })),
        )
        .await;
    assert_eq!(status, 423, "cli count while session locked: {body}");
    assert_eq!(body["code"], "session_locked", "{body}");

    println!("ARTIFACTS={}", rec.dir.display());
}
