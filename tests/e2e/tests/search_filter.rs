//! E2E tests for CLI search filters over seeded clipboard history.
//!
//! `dev seed-clipboard` requires the daemon to be stopped because it opens an
//! in-process application session over the same profile. Search runs through
//! the daemon client path, so this test seeds first, then restarts the daemon
//! over the same profile for querying.

use serde_json::Value;
use uc_e2e_tests::{TestCli, TestDaemon, TestProfile};

const PASSPHRASE: &str = "search-filter-passphrase";

fn seed_entry(cli: &TestCli, text: &str) -> String {
    let out = cli.run_capture(&["dev", "seed-clipboard", "--text", text]);
    assert!(
        out.success(),
        "seed failed (exit={}): stdout={}, stderr={}",
        out.exit_code,
        out.stdout,
        out.stderr
    );

    out.stdout
        .lines()
        .find_map(|line| line.strip_prefix("SEED_ENTRY_ID="))
        .map(str::to_string)
        .unwrap_or_else(|| panic!("seed output missing SEED_ENTRY_ID: {}", out.stdout))
}

fn search_json(cli: &TestCli, args: &[&str]) -> Value {
    let mut full_args = vec!["--json", "search"];
    full_args.extend_from_slice(args);

    let out = cli.run_capture(&full_args);
    assert!(
        out.success(),
        "search {:?} failed (exit={}): stdout={}, stderr={}",
        args,
        out.exit_code,
        out.stdout,
        out.stderr
    );
    serde_json::from_str(out.stdout.trim())
        .unwrap_or_else(|e| panic!("search output was not JSON: {e}\nstdout={}", out.stdout))
}

fn wait_for_search_ready(cli: &TestCli) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);

    loop {
        let out = cli.run_capture(&["--json", "search", "status"]);
        assert!(
            out.success(),
            "search status failed (exit={}): stdout={}, stderr={}",
            out.exit_code,
            out.stdout,
            out.stderr
        );
        let status: Value = serde_json::from_str(out.stdout.trim()).unwrap_or_else(|e| {
            panic!(
                "search status output was not JSON: {e}\nstdout={}",
                out.stdout
            )
        });
        if status
            .get("state")
            .and_then(Value::as_str)
            .is_some_and(|state| state == "ready")
        {
            return;
        }

        assert!(
            std::time::Instant::now() < deadline,
            "search index did not become ready; last status: {}",
            out.stdout
        );
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}

fn result_items(page: &Value) -> &[Value] {
    page.get("data")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_else(|| panic!("search JSON missing data array: {page}"))
}

fn contains_entry(page: &Value, entry_id: &str) -> bool {
    result_items(page).iter().any(|item| {
        item.get("entry_id")
            .and_then(Value::as_str)
            .is_some_and(|id| id == entry_id)
    })
}

fn content_type_for<'a>(page: &'a Value, entry_id: &str) -> Option<&'a str> {
    result_items(page).iter().find_map(|item| {
        let id = item.get("entry_id").and_then(Value::as_str)?;
        (id == entry_id).then(|| item.get("content_type").and_then(Value::as_str))?
    })
}

async fn initialized_cli_for_seed(test_name: &str) -> (TestDaemon, TestCli) {
    let profile = TestProfile::new(test_name);
    let mut daemon = TestDaemon::start(profile).await.expect("daemon start");
    let cli = TestCli::new(&daemon.profile);

    let init = cli.run_capture(&[
        "init",
        "--passphrase",
        PASSPHRASE,
        "--device-name",
        "search-filter-node",
    ]);
    assert!(
        init.success(),
        "init failed (exit={}): stdout={}, stderr={}",
        init.exit_code,
        init.stdout,
        init.stderr
    );

    daemon.kill();
    (daemon, cli)
}

#[tokio::test]
#[ignore]
async fn seeded_entries_support_type_and_tag_search_filters() {
    let (mut daemon, cli) = initialized_cli_for_seed("search-filters").await;
    let text_entry_id = seed_entry(&cli, "plain text search filter seed");
    let link_entry_id = seed_entry(&cli, "https://example.com/uniclip-e2e");
    let code_entry_id = seed_entry(
        &cli,
        "function greet(name) {\n  return `hello ${name}`;\n}",
    );

    daemon.restart().await.expect("daemon restart after seed");
    wait_for_search_ready(&cli);

    let text_page = search_json(&cli, &["--type", "text"]);
    assert!(
        contains_entry(&text_page, &text_entry_id),
        "text filter should return seeded entry {text_entry_id}: {text_page}"
    );
    assert_eq!(content_type_for(&text_page, &text_entry_id), Some("text"));

    let image_page = search_json(&cli, &["--type", "image"]);
    assert!(
        !contains_entry(&image_page, &text_entry_id),
        "image filter should not return text entry {text_entry_id}: {image_page}"
    );

    let link_page = search_json(&cli, &["--tag", "link"]);
    assert!(
        contains_entry(&link_page, &link_entry_id),
        "link tag should return seeded URL entry {link_entry_id}: {link_page}"
    );
    assert_eq!(content_type_for(&link_page, &link_entry_id), Some("text"));

    let code_page = search_json(&cli, &["--tag", "code"]);
    assert!(
        contains_entry(&code_page, &code_entry_id),
        "code tag should return seeded code entry {code_entry_id}: {code_page}"
    );
    assert_eq!(content_type_for(&code_page, &code_entry_id), Some("text"));
}
