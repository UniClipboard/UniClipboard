use uc_engine::StartupProgress;
use uc_webserver::api::startup::StartupServer;

#[tokio::test]
async fn startup_is_authenticated_available_before_engine_and_retains_interruption() {
    let root = std::env::temp_dir().join(format!("uc-startup-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("startup.conn");
    let (input, progress) = StartupProgress::channel();
    let token = uc_webserver::api::auth::load_or_create_auth_token_from_conn(&path).unwrap();
    let server = StartupServer::bind(progress, token.clone(), path.clone())
        .await
        .unwrap();
    let conn = uc_daemon_local::socket::read_daemon_conn_file_at(&path)
        .unwrap()
        .unwrap();
    let url = format!("http://127.0.0.1:{}/startup", conn.port);
    let client = reqwest::Client::new();
    assert_eq!(client.get(&url).send().await.unwrap().status(), 401);
    let state: serde_json::Value = client
        .get(&url)
        .bearer_auth(token.as_str())
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(state["progress"]["state"], "preparing");
    assert_eq!(state["service_ready"], false);
    assert_eq!(
        client
            .get(format!("http://127.0.0.1:{}/health", conn.port))
            .send()
            .await
            .unwrap()
            .status(),
        503
    );
    drop(input);
    let state: serde_json::Value = client
        .get(&url)
        .bearer_auth(token.as_str())
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(state["progress"]["state"], "interrupted");
    assert_eq!(state["progress"]["allowed_actions"]["retry"], true);
    server.shutdown().await.unwrap();
    assert!(!path.exists());
    std::fs::remove_dir_all(root).unwrap();
}
