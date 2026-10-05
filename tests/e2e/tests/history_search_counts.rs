//! E2E: History search candidate counts, zero-result relaxations and the
//! content-lock boundary of `POST /search/count`, against a real daemon.
//!
//! Isolation: a fresh `e2e-history-search-*` profile, daemon in server mode
//! (system clipboard replaced by a no-op, so ingest write-back never reaches the
//! user's clipboard), no rendezvous, no GUI.
//!
//! Content enters through real ingestion paths only: files through the local
//! capture pipeline (`uniclip dev capture-files`, daemon stopped, then the
//! production `POST /search/rebuild`), texts through the mobile LAN ingest
//! endpoint (`PUT /SyncClipboard.json` with the Basic credentials from
//! `uniclip mobile setup`), indexed live. The mobile listener binds
//! `0.0.0.0:<random free port>` (it has no loopback-only option) for the test's
//! duration and requires those credentials.
//!
//! The count queries below are the exact wire params the History page builds
//! (`buildCandidateCountQueries` / `buildRelaxationQueries` in
//! `apps/gui/src/components/history/composite-search/composite-search-model.ts`).
//!
//! Optional browser step: set `UC_E2E_HISTORY_BROWSER=1` to run
//! `apps/gui/e2e/history-search-browser.mjs` (the repo's webdriverio + the
//! local Google Chrome, headless) against this daemon, in two separately
//! reported phases: the search components alone, then the complete frontend
//! on `/history` with only the Tauri native layer stubbed.
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

/// `{ data }` of a 200 response, or a panic naming the step.
fn data(label: &str, (status, body): (u16, Value)) -> Value {
    assert_eq!(status, 200, "{label}: {body}");
    body["data"].clone()
}

async fn list_tags(api: &Api, rec: &mut Recorder, token: &str, label: &str) -> Value {
    let response = api
        .call(
            rec,
            label,
            token,
            reqwest::Method::GET,
            "/history/tags",
            None,
            None,
        )
        .await;
    data(label, response)
}

async fn add_tag(
    api: &Api,
    rec: &mut Recorder,
    token: &str,
    label: &str,
    tag: &str,
    entries: Value,
) -> Value {
    let path = format!("/history/tags/{tag}/entries/add");
    let body = json!({ "entryIds": entries });
    let response = api
        .call(
            rec,
            label,
            token,
            reqwest::Method::POST,
            &path,
            None,
            Some(&body),
        )
        .await;
    data(label, response)
}

/// Local history tags over HTTP against the real Engine: create-or-get, batch
/// add/remove with a missing entry, selection summary, search filter/count/
/// facet, rename conflict, merge, delete, and the error mapping. Leaves no tag
/// behind so later steps see the seeded state.
async fn history_tags_lifecycle(api: &Api, rec: &mut Recorder, token: &str) {
    use reqwest::Method;
    let tags = "/history/tags";
    assert_eq!(
        list_tags(api, rec, token, "tags-list-empty").await,
        json!([])
    );

    let created = data(
        "tags-create",
        api.call(
            rec,
            "tags-create",
            token,
            Method::POST,
            tags,
            None,
            Some(&json!({ "name": "Deploy" })),
        )
        .await,
    );
    assert_eq!(created["created"], true, "{created}");
    assert_eq!(created["tag"]["name"], "Deploy");
    let deploy = created["tag"]["tagId"].as_str().unwrap().to_string();
    let again = data(
        "tags-create-same-name",
        api.call(
            rec,
            "tags-create-same-name",
            token,
            Method::POST,
            tags,
            None,
            Some(&json!({ "name": "  deploy " })),
        )
        .await,
    );
    assert_eq!(
        again["created"], false,
        "trimmed, case-insensitive create-or-get: {again}"
    );
    assert_eq!(again["tag"]["tagId"], deploy.as_str());
    let (status, body) = api
        .call(
            rec,
            "tags-create-empty",
            token,
            Method::POST,
            tags,
            None,
            Some(&json!({ "name": "   " })),
        )
        .await;
    assert_eq!(status, 400, "empty name: {body}");

    let texts = data(
        "tags-query-texts",
        api.call(
            rec,
            "tags-query-texts",
            token,
            Method::GET,
            "/search/query",
            Some(&json!({ "query": "", "contentTypes": "text" })),
            None,
        )
        .await,
    );
    let mut ids: Vec<String> = texts["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["entryId"].as_str().unwrap().to_string())
        .collect();
    ids.sort();
    assert_eq!(ids.len(), TEXTS.len());
    let (t0, t1, t2) = (ids[0].clone(), ids[1].clone(), ids[2].clone());

    let batch = add_tag(
        api,
        rec,
        token,
        "tags-add-two-and-missing",
        &deploy,
        json!([t0, t1, "missing-entry"]),
    )
    .await;
    assert_eq!(
        batch,
        json!({ "changed": 2, "unchanged": 0, "missingEntryIds": ["missing-entry"] })
    );
    let batch = add_tag(api, rec, token, "tags-add-again", &deploy, json!([t0])).await;
    assert_eq!(
        batch,
        json!({ "changed": 0, "unchanged": 1, "missingEntryIds": [] })
    );

    let summary = data(
        "tags-summary",
        api.call(
            rec,
            "tags-summary",
            token,
            Method::POST,
            "/history/tags/summary",
            None,
            Some(&json!({ "entryIds": [t0, t1, t2] })),
        )
        .await,
    );
    assert_eq!(
        summary,
        json!({ "selected": 3, "tags": [{ "tagId": deploy, "applied": 2 }] })
    );

    // Search sees the association at once (query-time join, no index write).
    let filtered = data(
        "tags-search-filter",
        api.call(
            rec,
            "tags-search-filter",
            token,
            Method::GET,
            "/search/query",
            Some(&json!({ "query": "", "tags": deploy })),
            None,
        )
        .await,
    );
    assert_eq!(filtered["total"], 2, "{filtered}");
    assert!(filtered["items"]
        .as_array()
        .unwrap()
        .iter()
        .all(|item| item["tags"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t == deploy.as_str())));
    assert_eq!(
        counts(
            api,
            rec,
            "tags-search-count",
            token,
            &[json!({ "query": "", "tags": deploy })]
        )
        .await,
        vec![2]
    );
    let facet = data(
        "tags-search-facet",
        api.call(
            rec,
            "tags-search-facet",
            token,
            Method::GET,
            "/search/tags",
            None,
            None,
        )
        .await,
    );
    assert!(
        facet
            .as_array()
            .unwrap()
            .contains(&json!({ "tagId": deploy, "count": 2, "isBuiltin": false })),
        "custom tag in /search/tags: {facet}"
    );
    let listed = list_tags(api, rec, token, "tags-list-one").await;
    assert_eq!(listed[0]["entryCount"], 2, "{listed}");

    // Rename: a taken name is a 200 conflict, not a write.
    let release = data(
        "tags-create-release",
        api.call(
            rec,
            "tags-create-release",
            token,
            Method::POST,
            tags,
            None,
            Some(&json!({ "name": "Release" })),
        )
        .await,
    )["tag"]["tagId"]
        .as_str()
        .unwrap()
        .to_string();
    let rename_path = format!("{tags}/{release}");
    let conflict = data(
        "tags-rename-conflict",
        api.call(
            rec,
            "tags-rename-conflict",
            token,
            Method::PATCH,
            &rename_path,
            None,
            Some(&json!({ "name": "DEPLOY" })),
        )
        .await,
    );
    assert_eq!(
        conflict,
        json!({ "kind": "name_conflict", "existingTagId": deploy })
    );
    let renamed = data(
        "tags-rename",
        api.call(
            rec,
            "tags-rename",
            token,
            Method::PATCH,
            &rename_path,
            None,
            Some(&json!({ "name": "Hotfix" })),
        )
        .await,
    );
    assert_eq!(renamed["kind"], "renamed", "{renamed}");
    assert_eq!(renamed["tag"]["name"], "Hotfix");

    // Merge Hotfix (t1, t2) into Deploy (t0, t1).
    add_tag(
        api,
        rec,
        token,
        "tags-add-hotfix",
        &release,
        json!([t1, t2]),
    )
    .await;
    let merge_path = format!("{tags}/{deploy}/merge");
    let merged = data(
        "tags-merge",
        api.call(
            rec,
            "tags-merge",
            token,
            Method::POST,
            &merge_path,
            None,
            Some(&json!({ "sourceTagIds": [release] })),
        )
        .await,
    );
    assert_eq!(merged, json!({ "moved": 1, "alreadyOnTarget": 1 }));
    let listed = list_tags(api, rec, token, "tags-list-after-merge").await;
    assert_eq!(listed.as_array().unwrap().len(), 1, "{listed}");
    assert_eq!(listed[0]["entryCount"], 3, "{listed}");

    // Error mapping.
    let (status, body) = api
        .call(
            rec,
            "tags-merge-into-itself",
            token,
            Method::POST,
            &merge_path,
            None,
            Some(&json!({ "sourceTagIds": [deploy] })),
        )
        .await;
    assert_eq!(status, 400, "target among sources: {body}");
    let (status, body) = api
        .call(
            rec,
            "tags-add-unknown-tag",
            token,
            Method::POST,
            "/history/tags/no-such-tag/entries/add",
            None,
            Some(&json!({ "entryIds": [t0] })),
        )
        .await;
    assert_eq!(status, 404, "unknown tag: {body}");
    let (status, body) = api
        .call(
            rec,
            "tags-add-empty-batch",
            token,
            Method::POST,
            &format!("{tags}/{deploy}/entries/add"),
            None,
            Some(&json!({ "entryIds": [] })),
        )
        .await;
    assert_eq!(status, 400, "empty batch: {body}");

    let removed = data(
        "tags-remove",
        api.call(
            rec,
            "tags-remove",
            token,
            Method::POST,
            &format!("{tags}/{deploy}/entries/remove"),
            None,
            Some(&json!({ "entryIds": [t0] })),
        )
        .await,
    );
    assert_eq!(
        removed,
        json!({ "changed": 1, "unchanged": 0, "missingEntryIds": [] })
    );
    let deleted = data(
        "tags-delete",
        api.call(
            rec,
            "tags-delete",
            token,
            Method::DELETE,
            &format!("{tags}/{deploy}"),
            None,
            None,
        )
        .await,
    );
    assert_eq!(deleted, json!({ "detached": 2 }));
    assert_eq!(
        list_tags(api, rec, token, "tags-list-after-delete").await,
        json!([])
    );
    assert_eq!(
        counts(
            api,
            rec,
            "tags-entries-kept",
            token,
            &[json!({ "query": "" })]
        )
        .await,
        vec![(TEXTS.len() + FILES.len()) as u64],
        "deleting a tag keeps its entries"
    );
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
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

    // ── Files: local capture pipeline, daemon stopped (dev commands refuse otherwise).
    daemon.kill();
    let files_dir = daemon.profile.data_dir().join("e2e-seed-files");
    std::fs::create_dir_all(&files_dir).unwrap();
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

    // ── Texts: the mobile LAN ingest endpoint, indexed live.
    let mobile_port = free_port();
    let setup: Value = serde_json::from_str(
        cli_ok(
            &cli,
            &[
                "--json",
                "mobile",
                "setup",
                "--non-interactive",
                "--label",
                "e2e-phone",
                "--ip",
                "127.0.0.1",
                "--accept-network-risk",
                "--port",
                &mobile_port.to_string(),
            ],
        )
        .trim(),
    )
    .expect("mobile setup json");
    let username = setup["username"].as_str().expect("username").to_string();
    let password = setup["password"].as_str().expect("password").to_string();
    let mobile_url = format!("http://127.0.0.1:{mobile_port}/SyncClipboard.json");
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let ready = api
            .client
            .get(&mobile_url)
            .basic_auth(&username, Some(&password))
            .send()
            .await
            .is_ok_and(|r| r.status().is_success());
        if ready {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "mobile LAN listener never came up"
        );
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    for text in TEXTS {
        let doc = json!({ "type": "Text", "text": text, "hasData": false });
        let resp = api
            .client
            .put(&mobile_url)
            .basic_auth(&username, Some(&password))
            .json(&doc)
            .send()
            .await
            .expect("mobile put");
        let status = resp.status().as_u16();
        let body = resp.text().await.unwrap_or_default();
        rec.log.push(json!({
            "label": "mobile-put-text",
            "method": "PUT",
            "path": "/SyncClipboard.json (mobile LAN, Basic auth redacted)",
            "body": doc,
            "status": status,
            "response": body,
        }));
        assert!(
            (200..300).contains(&status),
            "mobile put {text}: {status} {body}"
        );
    }
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let all = total(
            &api,
            &mut rec,
            "query-all-after-ingest",
            &cli_token,
            &json!({ "query": "" }),
        )
        .await;
        if all == (TEXTS.len() + FILES.len()) as u64 {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "ingested texts never reached the index ({all})"
        );
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
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
    assert_eq!(
        type_counts,
        vec![TEXTS.len() as u64, 0, 0, FILES.len() as u64],
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

    // Relaxing either chip helps, by different amounts.
    let zero_three = json!({ "query": "", "contentTypes": "text", "extensions": "md" });
    assert_eq!(
        total(
            &api,
            &mut rec,
            "query-zero-result-3",
            &cli_token,
            &zero_three
        )
        .await,
        0
    );
    let relax_three = [
        json!({ "query": "", "extensions": "md" }), // drop type
        json!({ "query": "", "contentTypes": "text" }), // drop ext
    ];
    let relax_three_counts = counts(
        &api,
        &mut rec,
        "count-relaxations-3",
        &cli_token,
        &relax_three,
    )
    .await;
    assert_eq!(relax_three_counts, vec![1, TEXTS.len() as u64]);
    for (params, count) in relax_three.iter().zip(&relax_three_counts) {
        let listed = total(&api, &mut rec, "query-relaxation", &cli_token, params).await;
        assert_eq!(
            *count, listed,
            "relaxation count must equal the list total for {params}"
        );
    }

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

    // ── Local history tags over HTTP.
    history_tags_lifecycle(&api, &mut rec, &cli_token).await;

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
            .arg("e2e/history-search-browser.mjs")
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
    let (status, body) = api
        .call(
            &mut rec,
            "gui-tags-while-content-locked",
            &gui_token,
            reqwest::Method::GET,
            "/history/tags",
            None,
            None,
        )
        .await;
    assert_eq!(status, 423, "gui tags while content locked: {body}");
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
    // Engine code 1405 maps to the same 423 session_locked.
    let (status, body) = api
        .call(
            &mut rec,
            "cli-tags-while-session-locked",
            &cli_token,
            reqwest::Method::POST,
            "/history/tags",
            None,
            Some(&json!({ "name": "Locked" })),
        )
        .await;
    assert_eq!(status, 423, "cli tag create while session locked: {body}");
    assert_eq!(body["code"], "session_locked", "{body}");

    println!("ARTIFACTS={}", rec.dir.display());
}
