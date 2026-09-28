//! Diagnostic export of log files that another process still holds open.
//!
//! The daemon and Engine keep their current log files open for appending while
//! a diagnostic package is exported. On Windows the size recorded in the
//! directory entry lags behind the size seen through an open handle until the
//! writer closes the file, so these tests use a separate writer process and
//! keep its handle open across the export. The test process itself must not
//! open a log before exporting it: opening a file makes NTFS refresh its
//! directory entry, which would hide the stale size.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::print_stderr)]

use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;

use uc_observability::startup_logs::{
    export_diagnostic_logs, DiagnosticArchiveMode, DiagnosticArchiveReport,
    DiagnosticArchiveRequest,
};

const WRITER_PATH_ENV: &str = "UC_DIAGNOSTIC_EXPORT_TEST_WRITER_PATH";
const WRITER_MODE_ENV: &str = "UC_DIAGNOSTIC_EXPORT_TEST_WRITER_MODE";
const WRITER_RECORDS_ENV: &str = "UC_DIAGNOSTIC_EXPORT_TEST_WRITER_RECORDS";
const READY_PREFIX: &str = "log-writer-ready ";
const EXPORT_DEADLINE: Duration = Duration::from_secs(60);

/// Writer side of the tests. It only acts when started by one of the tests
/// below; a normal test run returns immediately.
#[test]
fn log_writer_process() {
    let Ok(path) = std::env::var(WRITER_PATH_ENV) else {
        return;
    };
    let mode = std::env::var(WRITER_MODE_ENV).unwrap();
    let records: u64 = std::env::var(WRITER_RECORDS_ENV).unwrap().parse().unwrap();
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .unwrap();
    let mut written = 0u64;
    for sequence in 0..records {
        written += write_record(&mut file, "held", sequence);
    }
    file.flush().unwrap();

    let stdin_closed = Arc::new(AtomicBool::new(false));
    let watcher = {
        let stdin_closed = Arc::clone(&stdin_closed);
        std::thread::spawn(move || {
            let _ = std::io::stdin().read_to_end(&mut Vec::new());
            stdin_closed.store(true, Ordering::SeqCst);
        })
    };
    let mut stdout = std::io::stdout();
    writeln!(stdout, "{READY_PREFIX}{written}").unwrap();
    stdout.flush().unwrap();

    match mode.as_str() {
        // Keep the handle open, without further writes, until the test ends.
        "hold" => {}
        // Keep appending as fast as possible until the test ends.
        "stream" => {
            let mut sequence = 0u64;
            while !stdin_closed.load(Ordering::SeqCst) {
                write_record(&mut file, "streamed", sequence);
                sequence += 1;
            }
        }
        other => panic!("unknown writer mode {other}"),
    }
    watcher.join().unwrap();
    drop(file);
}

#[test]
fn export_includes_a_log_created_and_still_held_open_by_another_process() {
    let root = tempfile::tempdir().unwrap();
    let logs = root.path().join("logs");
    fs::create_dir(&logs).unwrap();
    let log = logs.join("engine.2026-09-28.jsonl");

    let writer = LogWriter::start(&log, "hold", 1_500);
    let written = writer.ready_bytes;
    report_directory_view(&logs, &log);

    let (report, archived) = export_online(&logs, root.path(), "engine.2026-09-28.jsonl");
    writer.finish();
    let expected = fs::read(&log).unwrap();
    assert_eq!(expected.len() as u64, written);

    assert_eq!(
        archived.len(),
        expected.len(),
        "archived bytes must match the open file"
    );
    assert_eq!(archived, expected);
    assert_eq!(report.included_files, ["engine.2026-09-28.jsonl"]);
    assert!(report.truncated_files.is_empty());
    assert!(report.unreadable_files.is_empty());
}

#[test]
fn export_includes_records_appended_after_the_directory_entry_was_last_updated() {
    let root = tempfile::tempdir().unwrap();
    let logs = root.path().join("logs");
    fs::create_dir(&logs).unwrap();
    let log = logs.join("uniclipboard-daemon.json.2026-09-28");
    let mut earlier_run = fs::File::create(&log).unwrap();
    for sequence in 0..200 {
        write_record(&mut earlier_run, "earlier-run", sequence);
    }
    drop(earlier_run);
    let earlier_length = fs::metadata(&log).unwrap().len();

    let writer = LogWriter::start(&log, "hold", 1_500);
    let written = earlier_length + writer.ready_bytes;
    report_directory_view(&logs, &log);

    let (report, archived) =
        export_online(&logs, root.path(), "uniclipboard-daemon.json.2026-09-28");
    writer.finish();
    let expected = fs::read(&log).unwrap();
    assert_eq!(expected.len() as u64, written);

    assert_eq!(
        archived.len(),
        expected.len(),
        "archived bytes must include the current run"
    );
    assert_eq!(archived, expected);
    assert!(String::from_utf8(archived)
        .unwrap()
        .lines()
        .last()
        .unwrap()
        .contains("\"run\":\"held\""));
    assert!(report.truncated_files.is_empty());
    assert!(report.unreadable_files.is_empty());
}

#[test]
fn export_takes_a_bounded_snapshot_while_another_process_keeps_appending() {
    let root = tempfile::tempdir().unwrap();
    let logs = root.path().join("logs");
    fs::create_dir(&logs).unwrap();
    let log = logs.join("engine.2026-09-28.jsonl");

    let writer = LogWriter::start(&log, "stream", 1_500);
    // Everything the writer reported before the export starts must be in the
    // package.
    let written_before_export = writer.ready_bytes;
    report_directory_view(&logs, &log);

    let (report, archived) = export_online(&logs, root.path(), "engine.2026-09-28.jsonl");
    writer.finish();
    let final_content = fs::read(&log).unwrap();

    eprintln!(
        "streaming export: before_export={written_before_export} archived={} final={}",
        archived.len(),
        final_content.len()
    );
    assert!(
        archived.len() as u64 >= written_before_export,
        "archived {} bytes, but {written_before_export} bytes were written before the export",
        archived.len()
    );
    assert!(
        final_content.starts_with(&archived),
        "the archive must be a prefix of the log"
    );
    assert!(report.truncated_files.is_empty());
    assert!(report.unreadable_files.is_empty());
}

struct LogWriter {
    child: Child,
    stdin: ChildStdin,
    ready_bytes: u64,
}

impl LogWriter {
    fn start(path: &Path, mode: &str, records: u64) -> Self {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "log_writer_process", "--nocapture"])
            .env(WRITER_PATH_ENV, path)
            .env(WRITER_MODE_ENV, mode)
            .env(WRITER_RECORDS_ENV, records.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (ready_sender, ready) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { return };
                if let Some(bytes) = line.strip_prefix(READY_PREFIX) {
                    let _ = ready_sender.send(bytes.trim().parse::<u64>().unwrap());
                }
            }
        });
        let ready_bytes = ready
            .recv_timeout(EXPORT_DEADLINE)
            .expect("log writer process did not become ready");
        Self {
            child,
            stdin,
            ready_bytes,
        }
    }

    fn finish(mut self) {
        drop(self.stdin);
        let status = self.child.wait().unwrap();
        assert!(status.success(), "log writer process failed: {status}");
    }
}

fn write_record(file: &mut fs::File, run: &str, sequence: u64) -> u64 {
    let line = format!(
        "{{\"run\":\"{run}\",\"seq\":{sequence},\"message\":\"synthetic diagnostic record\"}}\n"
    );
    file.write_all(line.as_bytes()).unwrap();
    line.len() as u64
}

/// Record the directory-entry size without opening the file, so a test log
/// shows which size the old directory-based export would have copied.
fn report_directory_view(logs: &Path, path: &Path) {
    let name = path.file_name().unwrap();
    let directory_length = fs::read_dir(logs)
        .unwrap()
        .filter_map(Result::ok)
        .find(|entry| entry.file_name() == name)
        .map(|entry| entry.metadata().unwrap().len());
    eprintln!(
        "{}: directory_entry_len_before_export={directory_length:?}",
        name.to_string_lossy()
    );
}

fn export_online(
    logs: &Path,
    output_dir: &Path,
    log_name: &str,
) -> (DiagnosticArchiveReport, Vec<u8>) {
    let output: PathBuf = output_dir.join("support.zip");
    let logs_for_export = logs.to_path_buf();
    let output_for_export = output.clone();
    let (done, finished) = mpsc::channel();
    std::thread::spawn(move || {
        let result = export_diagnostic_logs(
            &logs_for_export,
            &output_for_export,
            DiagnosticArchiveRequest {
                mode: DiagnosticArchiveMode::Online,
                since: None,
                engine_preparation: None,
                startup_status: None,
            },
        );
        let _ = done.send(result);
    });
    let report = finished
        .recv_timeout(EXPORT_DEADLINE)
        .expect("diagnostic export did not finish while the log was being written")
        .unwrap();

    let mut archive = zip::ZipArchive::new(fs::File::open(&output).unwrap()).unwrap();
    let mut archived = Vec::new();
    archive
        .by_name(&format!("logs/{log_name}"))
        .unwrap()
        .read_to_end(&mut archived)
        .unwrap();
    let mut manifest = String::new();
    archive
        .by_name("manifest.json")
        .unwrap()
        .read_to_string(&mut manifest)
        .unwrap();
    let manifest: serde_json::Value = serde_json::from_str(&manifest).unwrap();
    assert_eq!(
        manifest["collection"],
        serde_json::to_value(&report).unwrap(),
        "manifest collection must match the returned report"
    );
    (report, archived)
}
