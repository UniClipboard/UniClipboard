//! E2E tests for CLI search filters over seeded clipboard history.
//!
//! `dev seed-clipboard` requires the daemon to be stopped because it opens an
//! in-process application session over the same profile. Search runs through
//! the daemon client path, so each test seeds first, then lets `search` spawn
//! or reuse a daemon for querying.

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
async fn search_type_text_matches_seeded_text_and_type_image_does_not() {
    let (_profile_guard, cli) = initialized_cli_for_seed("search-filter-type").await;
    let entry_id = seed_entry(&cli, "plain text search filter seed");

    let text_page = search_json(&cli, &["--type", "text"]);
    assert!(
        contains_entry(&text_page, &entry_id),
        "text filter should return seeded entry {entry_id}: {text_page}"
    );
    assert_eq!(content_type_for(&text_page, &entry_id), Some("text"));

    let image_page = search_json(&cli, &["--type", "image"]);
    assert!(
        !contains_entry(&image_page, &entry_id),
        "image filter should not return text entry {entry_id}: {image_page}"
    );
}

#[tokio::test]
#[ignore]
async fn search_tag_link_matches_seeded_url_text() {
    let (_profile_guard, cli) = initialized_cli_for_seed("search-filter-link").await;
    let entry_id = seed_entry(&cli, "link seed https://example.com/uniclip-e2e");

    let page = search_json(&cli, &["--tag", "link"]);
    assert!(
        contains_entry(&page, &entry_id),
        "link tag should return seeded URL entry {entry_id}: {page}"
    );
    assert_eq!(content_type_for(&page, &entry_id), Some("text"));
}

#[tokio::test]
#[ignore]
async fn search_tag_code_matches_seeded_code_like_text() {
    let (_profile_guard, cli) = initialized_cli_for_seed("search-filter-code").await;
    let entry_id = seed_entry(
        &cli,
        "function greet(name) {\n  return `hello ${name}`;\n}",
    );

    let page = search_json(&cli, &["--tag", "code"]);
    assert!(
        contains_entry(&page, &entry_id),
        "code tag should return seeded code entry {entry_id}: {page}"
    );
    assert_eq!(content_type_for(&page, &entry_id), Some("text"));
}
