//! The managed log file names are defined twice, in Go (offline export from the
//! GUI) and in Rust (daemon export). Both are checked against one fixture.

use std::path::PathBuf;

use serde::Deserialize;
use uc_observability::startup_logs::{managed_log_source, source_written_by_current_host};

#[derive(Deserialize)]
struct Fixture {
    roles: std::collections::BTreeMap<String, Role>,
    #[serde(rename = "undatedFiles")]
    undated_files: Vec<String>,
}

#[derive(Deserialize)]
struct Role {
    #[serde(rename = "writtenByCurrentHost")]
    written_by_current_host: bool,
    prefix: String,
    suffix: String,
    positive: Vec<String>,
    negative: Vec<String>,
}

fn fixture() -> Fixture {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../packages/desktop-host-go/diaglogs/testdata/log-source-patterns.json");
    serde_json::from_slice(&std::fs::read(path).expect("shared fixture")).expect("fixture json")
}

#[test]
fn every_positive_name_belongs_to_its_role() {
    for (role, entry) in fixture().roles {
        assert!(!entry.positive.is_empty(), "{role}");
        for name in entry.positive {
            assert!(name.starts_with(&entry.prefix) && name.ends_with(&entry.suffix));
            assert_eq!(managed_log_source(&name), Some(role.as_str()), "{name}");
        }
    }
}

#[test]
fn no_negative_name_belongs_to_any_role() {
    for (role, entry) in fixture().roles {
        for name in entry.negative {
            assert_eq!(managed_log_source(&name), None, "{role}: {name}");
        }
    }
}

#[test]
fn undated_files_are_not_dated_log_roles() {
    let undated = fixture().undated_files;
    assert!(undated.contains(&"daemon-startup-failure.json".to_string()));
    for name in undated {
        assert_eq!(managed_log_source(&name), None, "{name}");
    }
}

#[test]
fn only_roles_a_current_process_writes_can_be_reported_missing() {
    for (role, entry) in fixture().roles {
        assert_eq!(
            source_written_by_current_host(&role),
            Some(entry.written_by_current_host),
            "{role}"
        );
    }
}
