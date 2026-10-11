//! Real headless daemon LAN activation -> SSE -> document-pull evidence.
use serde_json::{json, Value};
use std::time::{Duration, Instant};
use uc_e2e_tests::{TestCli, TestDaemon, TestProfile};

struct Subscriber {
    response: reqwest::Response,
    buffered: String,
}
impl Subscriber {
    async fn connect(client: &reqwest::Client, url: &str, user: &str, pass: &str) -> Self {
        let response = client
            .get(format!("{url}/api/sse/clipboard"))
            .basic_auth(user, Some(pass))
            .send()
            .await
            .expect("SSE connect");
        assert!(response.status().is_success());
        Self {
            response,
            buffered: String::new(),
        }
    }
    async fn collect(&mut self, window: Duration) -> Vec<Value> {
        let deadline = tokio::time::Instant::now() + window;
        let mut events = Vec::new();
        loop {
            while let Some(end) = self.buffered.find("\n\n") {
                let frame: String = self.buffered.drain(..end + 2).collect();
                let name = frame
                    .lines()
                    .find_map(|l| l.strip_prefix("event:").map(str::trim));
                let data = frame
                    .lines()
                    .find_map(|l| l.strip_prefix("data:").map(str::trim));
                if let (Some(name), Some(data)) = (name, data) {
                    let value: Value = serde_json::from_str(data).expect("SSE JSON");
                    events.push(json!({"event":name,"data":value}));
                }
            }
            match tokio::time::timeout_at(deadline, self.response.chunk()).await {
                Err(_) => return events,
                Ok(Ok(Some(bytes))) => self
                    .buffered
                    .push_str(std::str::from_utf8(&bytes).expect("UTF8")),
                _ => panic!("SSE ended unexpectedly"),
            }
        }
    }
}

#[tokio::test]
#[ignore]
async fn mobile_lan_activation_notifications() {
    let artifact = std::env::var_os("UC_LAN_EVENT_ARTIFACT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("target/evidence")
                .join(format!("mobile-lan-events-{}.json", uuid::Uuid::new_v4()))
        });
    if let Some(parent) = artifact.parent() {
        std::fs::create_dir_all(parent).expect("artifact directory");
    }
    let daemon = TestDaemon::start(TestProfile::new("lan-events"))
        .await
        .expect("daemon");
    let cli = TestCli::new(&daemon.profile);
    let init = cli.run_capture(&[
        "init",
        "--passphrase",
        "synthetic-lan-pass",
        "--device-name",
        "synthetic-node",
    ]);
    assert!(init.success(), "init failed");
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let setup = cli.run_capture(&[
        "--json",
        "mobile",
        "setup",
        "--non-interactive",
        "--label",
        "synthetic-phone",
        "--ip",
        "127.0.0.1",
        "--accept-network-risk",
        "--port",
        &port.to_string(),
    ]);
    assert!(setup.success(), "LAN setup failed");
    let credentials: Value = serde_json::from_str(setup.stdout.trim()).unwrap();
    let user = credentials["username"].as_str().unwrap();
    let pass = credentials["password"].as_str().unwrap();
    let url = format!("http://127.0.0.1:{port}");
    let client = reqwest::Client::new();
    let mut subscribers = vec![
        Subscriber::connect(&client, &url, user, pass).await,
        Subscriber::connect(&client, &url, user, pass).await,
    ];
    let mut timeline = Vec::new();
    let mut passed = true;
    let mut get_count = 0;
    for (index, subscriber) in subscribers.iter_mut().enumerate() {
        let events = subscriber.collect(Duration::from_millis(300)).await;
        passed &= events.len() == 1 && events[0]["event"] == "hello";
        let response = client
            .get(format!("{url}/SyncClipboard.json"))
            .basic_auth(user, Some(pass))
            .send()
            .await
            .unwrap();
        assert!(response.status().is_success());
        let _: Value = response.json().await.unwrap();
        get_count += 1;
        timeline.push(json!({"step":"initial","subscriber":index,"events":events,"gets":1}));
    }
    let mut content_ids = Vec::new();
    // A -> B -> A, followed by deliberate reactivation of the same A.
    for (step, text) in ["synthetic-A", "synthetic-B", "synthetic-A", "synthetic-A"]
        .iter()
        .enumerate()
    {
        let started = Instant::now();
        let response = client
            .put(format!("{url}/SyncClipboard.json"))
            .basic_auth(user, Some(pass))
            .json(&json!({"type":"Text","text":text,"hasData":false,"size":text.len()}))
            .send()
            .await
            .unwrap();
        assert!(
            response.status().is_success(),
            "PUT status {}",
            response.status()
        );
        let put_ms = started.elapsed().as_millis();
        for (index, subscriber) in subscribers.iter_mut().enumerate() {
            let events = subscriber.collect(Duration::from_millis(700)).await;
            let updates: Vec<_> = events.iter().filter(|e| e["event"] == "update").collect();
            passed &= updates.len() == 1;
            let mut pulls = Vec::new();
            for event in &updates {
                let started = Instant::now();
                let response = client
                    .get(format!("{url}/SyncClipboard.json"))
                    .basic_auth(user, Some(pass))
                    .send()
                    .await
                    .unwrap();
                assert!(response.status().is_success());
                let doc: Value = response.json().await.unwrap();
                let matches =
                    doc["text"] == *text && doc["contentId"] == event["data"]["content_id"];
                passed &= matches;
                if index == 0 {
                    content_ids.push(doc["contentId"].clone());
                }
                pulls.push(json!({"ms":started.elapsed().as_millis(),"matches":matches}));
                get_count += 1;
            }
            timeline.push(json!({"step":step,"subscriber":index,"putMs":put_ms,"events":events,"gets":pulls.len(),"pulls":pulls}));
        }
    }
    drop(subscribers.remove(0));
    let mut reconnect = Subscriber::connect(&client, &url, user, pass).await;
    let events = reconnect.collect(Duration::from_millis(300)).await;
    passed &= events.len() == 1 && events[0]["event"] == "hello";
    let doc: Value = client
        .get(format!("{url}/SyncClipboard.json"))
        .basic_auth(user, Some(pass))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    passed &= doc["text"] == "synthetic-A";
    get_count += 1;
    timeline.push(json!({"step":"reconnect","events":events,"gets":1,"currentMatches":doc["text"] == "synthetic-A"}));
    if content_ids.len() == 4 {
        passed &= content_ids[0] != content_ids[1]
            && content_ids[0] == content_ids[2]
            && content_ids[2] == content_ids[3];
    } else {
        passed = false;
    }
    let mut evidence = json!({"schema":1,"passed":passed,"transport":"headless-daemon-LAN-PUT-SSE-GET","gets":get_count,"expectedGets":11,"timeline":timeline,"notRun":["OS clipboard capture","physical device","file/image","lag/resync"]});
    let owned_paths = [
        daemon.profile.data_dir().clone(),
        daemon.profile.cache_dir().clone(),
        daemon.profile.log_dir().clone(),
        daemon.profile.upgrade_backup_dir().clone(),
    ];
    drop(reconnect);
    drop(subscribers);
    drop(daemon);
    let cleaned = owned_paths.iter().all(|path| !path.exists());
    evidence["cleanupVerified"] = json!(cleaned);
    std::fs::write(artifact, serde_json::to_vec_pretty(&evidence).unwrap()).unwrap();
    assert!(cleaned, "owned profile resources remain");
    assert!(
        passed,
        "LAN activation notification contract failed; see artifact"
    );
}
