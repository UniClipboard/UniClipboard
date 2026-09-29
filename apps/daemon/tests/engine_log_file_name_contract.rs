//! Contract between Desktop's local Engine log-name parser and the Engine's own
//! `managed_log_file_date`.
//!
//! Diagnostic export uses `uc_observability::startup_logs::engine_log_file_date`
//! so the GUI does not link the Engine. The daemon links both, so this is where
//! the two are compared: every name the Engine writes must be recognized as its
//! date, and the local parser must accept exactly what the Engine accepts.

use chrono::{Datelike, Duration, NaiveDate};
use uc_engine::observability::diagnostics::{managed_log_file_date, managed_log_file_name};
use uc_observability::startup_logs::engine_log_file_date;

#[test]
fn every_engine_generated_name_is_recognized_as_its_date() {
    let mut dates = Vec::new();
    for year in [2024, 2026] {
        let mut date = NaiveDate::from_ymd_opt(year, 1, 1).unwrap();
        while date.year() == year {
            dates.push(date);
            date += Duration::days(1);
        }
    }
    dates.extend([
        NaiveDate::from_ymd_opt(1, 1, 1).unwrap(),
        NaiveDate::from_ymd_opt(999, 12, 31).unwrap(),
        NaiveDate::from_ymd_opt(1970, 1, 1).unwrap(),
        NaiveDate::from_ymd_opt(2000, 2, 29).unwrap(),
        NaiveDate::from_ymd_opt(9999, 12, 31).unwrap(),
    ]);

    for date in dates {
        let name = managed_log_file_name(date);
        assert_eq!(
            engine_log_file_date(&name),
            Some(date),
            "Engine log {name:?} must be recognized"
        );
        assert_eq!(engine_log_file_date(&name), managed_log_file_date(&name));
    }
}

#[test]
fn local_parser_accepts_exactly_what_the_engine_accepts() {
    let names = [
        // Canonical and lenient forms the date parser may accept.
        "engine.2026-09-05.jsonl",
        "engine.2026-9-5.jsonl",
        "engine.+2026-09-05.jsonl",
        "engine. 2026-09-05.jsonl",
        "engine.2026- 09-05.jsonl",
        "engine.02026-09-05.jsonl",
        // Invalid calendar dates.
        "engine.2026-02-30.jsonl",
        "engine.2025-02-29.jsonl",
        "engine.2026-13-01.jsonl",
        "engine.2026-00-10.jsonl",
        "engine.2026-09-00.jsonl",
        // Names that are not Engine-managed daily logs.
        "",
        "engine.",
        ".jsonl",
        "engine..jsonl",
        "engine.latest.jsonl",
        "engine.2026-09-05",
        "engine.2026-09-05.json",
        "engine.2026-09-05.jsonl.1",
        "engine.2026-09-05.jsonl.gz",
        "engine.2026-09-05.jsonl ",
        " engine.2026-09-05.jsonl",
        "Engine.2026-09-05.jsonl",
        "engine.2026-09-05.JSONL",
        "engine-2026-09-05.jsonl",
        "engine.2026-09-05T00:00:00.jsonl",
        "engine.2026-09-05.2026-09-06.jsonl",
        "nested/engine.2026-09-05.jsonl",
        "logs\\engine.2026-09-05.jsonl",
        "engine.２０２６-09-05.jsonl",
        // Desktop role logs are recognized by the Desktop branch only.
        "uniclipboard-gui.json.2026-09-05",
        "uniclipboard-daemon.json.2026-09-05",
        "uniclipboard-cli.json.2026-09-05",
    ];

    for name in names {
        assert_eq!(
            engine_log_file_date(name),
            managed_log_file_date(name),
            "local and Engine parsers disagree on {name:?}"
        );
    }
}
