//! E2E tests for the daemon's built-in relay overview (`GET /settings/relay-overview`).
//!
//! They run the real `uniclipd` against an isolated profile and read the
//! Engine-owned overview over HTTP. The overview describes configuration
//! (`inEffect` means "the running node was configured with this relay"); no test
//! here claims, or can prove, relay connectivity.
//!
//! Run with: cargo test --manifest-path tests/e2e/Cargo.toml --test relay_overview -- --ignored

use serde_json::{json, Value};
use uc_e2e_tests::{get_session_token, TestCli, TestDaemon, TestProfile};

const BUILT_IN: [(&str, &str); 4] = [
    ("na-east", "https://use1-1.relay.n0.iroh.link./"),
    ("na-west", "https://usw1-1.relay.n0.iroh.link./"),
    ("eu", "https://euc1-1.relay.n0.iroh.link./"),
    ("asia-pacific", "https://aps1-1.relay.n0.iroh.link./"),
];
const CUSTOM_URL: &str = "https://relay.example.invalid/";

struct Context {
    daemon: TestDaemon,
    client: reqwest::Client,
}

impl Context {
    async fn start(name: &str) -> Self {
        let profile = TestProfile::new(name);
        let daemon = TestDaemon::start(profile).await.expect("daemon start");
        let cli = TestCli::new(&daemon.profile);
        let out = cli.run_capture(&[
            "init",
            "--passphrase",
            "relay-overview-pass",
            "--device-name",
            "relay-overview-node",
        ]);
        assert!(out.success(), "init failed: {}", out.stderr);
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .expect("http client");
        Self { daemon, client }
    }

    async fn token(&self) -> String {
        format!(
            "Session {}",
            get_session_token(&self.daemon, &self.client).await
        )
    }

    async fn get(&self, path: &str) -> Value {
        let resp = self
            .client
            .get(format!("{}{path}", self.daemon.base_url()))
            .header("Authorization", self.token().await)
            .send()
            .await
            .expect("GET request");
        assert!(resp.status().is_success(), "GET {path}: {}", resp.status());
        let body: Value = resp.json().await.expect("json body");
        body.get("data").cloned().unwrap_or(body)
    }

    async fn overview(&self) -> Value {
        self.get("/settings/relay-overview").await
    }

    async fn mutate_custom(&self, body: Value) -> Value {
        let resp = self
            .client
            .post(format!("{}/settings/custom-relays", self.daemon.base_url()))
            .header("Authorization", self.token().await)
            .json(&body)
            .send()
            .await
            .expect("POST custom-relays");
        assert!(resp.status().is_success(), "mutation: {}", resp.status());
        resp.json().await.expect("mutation json")
    }
}

fn entries(overview: &Value) -> &Vec<Value> {
    overview["entries"].as_array().expect("entries array")
}

fn assert_built_in_listed_first(overview: &Value) {
    let list = entries(overview);
    for (index, (region, url)) in BUILT_IN.iter().enumerate() {
        let entry = &list[index];
        assert_eq!(entry["source"], "builtIn", "entry {index}: {entry}");
        assert_eq!(entry["regionId"], *region);
        assert_eq!(entry["url"], *url);
        assert_eq!(
            entry["credentialConfigured"], false,
            "built-in never has a token"
        );
    }
}

#[tokio::test]
#[ignore]
async fn built_in_relays_are_listed_for_default_settings() {
    let ctx = Context::start("relay-overview-default").await;
    let overview = ctx.overview().await;

    assert_eq!(overview["savedMode"], "builtIn");
    assert_eq!(overview["changePending"], false, "{overview}");
    assert_eq!(entries(&overview).len(), BUILT_IN.len(), "{overview}");
    assert_built_in_listed_first(&overview);

    // Built-in relays are never copied into the user's custom relay settings.
    let settings = ctx.get("/settings").await;
    assert_eq!(
        settings["network"]["customRelayUrls"],
        json!([]),
        "built-in relays leaked into user settings: {settings}"
    );
    assert_eq!(ctx.get("/settings/custom-relays").await, json!([]));
}

#[tokio::test]
#[ignore]
async fn custom_relay_replaces_built_in_and_is_applied_after_restart() {
    let mut ctx = Context::start("relay-overview-custom").await;
    let applied_before = ctx.overview().await["appliedMode"].clone();

    ctx.mutate_custom(json!({
        "action": "add",
        "url": CUSTOM_URL,
        "credential": { "action": "keep" },
    }))
    .await;

    // Saved settings now say "custom", the built-in list is still listed but not used,
    // and Engine reports the change as pending instead of claiming it is applied.
    let saved = ctx.overview().await;
    assert_eq!(saved["savedMode"], "custom", "{saved}");
    assert_built_in_listed_first(&saved);
    let list = entries(&saved);
    assert_eq!(list.len(), BUILT_IN.len() + 1, "{saved}");
    assert_eq!(list[BUILT_IN.len()]["source"], "custom");
    assert_eq!(list[BUILT_IN.len()]["url"], CUSTOM_URL);
    // `inEffect` follows the RUNNING node, not the saved settings: until the node is rebuilt
    // the built-in relays stay in effect and the new custom relay is not.
    if applied_before == "builtIn" {
        assert_eq!(saved["changePending"], true, "{saved}");
        assert_eq!(saved["appliedMode"], "builtIn", "{saved}");
        assert!(
            list[..BUILT_IN.len()].iter().all(|e| e["inEffect"] == true),
            "{saved}"
        );
        assert_eq!(list[BUILT_IN.len()]["inEffect"], false, "{saved}");
    }
    let settings = ctx.get("/settings").await;
    assert_eq!(settings["network"]["customRelayUrls"], json!([CUSTOM_URL]));

    // A new node is built after a daemon restart; only then is the saved mode applied.
    ctx.daemon
        .restart_preserving()
        .await
        .expect("restart daemon");
    let applied = ctx.overview().await;
    assert_eq!(applied["savedMode"], "custom", "{applied}");
    assert_eq!(applied["appliedMode"], "custom", "{applied}");
    assert_eq!(applied["changePending"], false, "{applied}");
    let list = entries(&applied);
    assert!(list[..BUILT_IN.len()]
        .iter()
        .all(|e| e["inEffect"] == false));
    assert_eq!(list[BUILT_IN.len()]["inEffect"], true, "{applied}");

    // Removing the custom relay returns to the built-in list (pending until restart again).
    ctx.mutate_custom(json!({ "action": "delete", "url": CUSTOM_URL }))
        .await;
    let removed = ctx.overview().await;
    assert_eq!(removed["savedMode"], "builtIn", "{removed}");
    assert_eq!(removed["changePending"], true, "{removed}");
    assert_eq!(entries(&removed).len(), BUILT_IN.len());
    ctx.daemon
        .restart_preserving()
        .await
        .expect("restart daemon");
    let restored = ctx.overview().await;
    assert_eq!(restored["appliedMode"], "builtIn", "{restored}");
    assert_eq!(restored["changePending"], false, "{restored}");
    assert!(entries(&restored).iter().all(|e| e["inEffect"] == true));
}
