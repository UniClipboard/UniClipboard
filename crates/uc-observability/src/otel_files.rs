//! Bounded file-prefix collection for both live and offline diagnostics.
use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use zip::{write::SimpleFileOptions, ZipWriter};

const LINE_BYTES: usize = 64 * 1024;
const POLICY: &str = include_str!("../../../packages/desktop-host-go/diaglogs/log-attributes.json");

fn selection(root: &Path, signal: &str) -> (Vec<PathBuf>, Vec<Value>) {
    let mut paths = Vec::new();
    let mut issues = Vec::new();
    let directory = root.join("diagnostics/engine");
    match fs::read_dir(&directory) {
        Ok(runs) => {
            for run in runs {
                let run = match run {
                    Ok(run) => run,
                    Err(_) => {
                        issues.push(json!({"name":"diagnostics/engine", "state":"unreadable"}));
                        continue;
                    }
                };
                let name = run.file_name().to_string_lossy().into_owned();
                if name.len() != 36
                    || uuid::Uuid::parse_str(&name).is_err()
                    || !run.file_type().is_ok_and(|t| t.is_dir())
                {
                    continue;
                }
                let label = format!("diagnostics/engine/{name}");
                match fs::read_dir(run.path()) {
                Ok(entries) => for entry in entries {
                    match entry {
                        Ok(entry) if segment(&entry.file_name().to_string_lossy(),signal) => {
                            match entry.file_type() { Ok(t) if t.is_file() => paths.push(entry.path()), Ok(_) => {}, Err(_) => issues.push(json!({"name":label,"state":"unreadable"})) }
                        },
                        Ok(_) => {}, Err(_) => issues.push(json!({"name":label,"state":"unreadable"})),
                    }
                },
                Err(error) => issues.push(json!({"name":label,"state": if error.kind()==io::ErrorKind::NotFound {"missing"}else{"unreadable"}})),
            }
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(_) => issues.push(json!({"name":"diagnostics/engine", "state":"unreadable"})),
    }
    if signal == "logs" {
        if let Ok(entries) = fs::read_dir(root) {
            for entry in entries.flatten() {
                if crate::startup_logs::engine_log_file_date(&entry.file_name().to_string_lossy())
                    .is_some()
                    && entry.file_type().is_ok_and(|t| t.is_file())
                {
                    paths.push(entry.path());
                }
            }
        }
    }
    paths.sort();
    (paths, issues)
}
pub(crate) fn files(root: &Path, signal: &str) -> Vec<PathBuf> {
    selection(root, signal).0
}
fn segment(name: &str, signal: &str) -> bool {
    let Some(rest) = name
        .strip_prefix(&format!("{signal}."))
        .and_then(|n| n.strip_suffix(".jsonl"))
    else {
        return false;
    };
    let Some((date, sequence)) = rest.split_once('.') else {
        return false;
    };
    date.len() == 8
        && date.bytes().all(|b| b.is_ascii_digit())
        && (10..=20).contains(&sequence.len())
        && sequence.bytes().all(|b| b.is_ascii_digit())
}
pub(crate) fn has_files(root: &Path) -> bool {
    ["metrics", "logs", "traces"].iter().any(|signal| {
        let (paths, issues) = selection(root, signal);
        !paths.is_empty() || !issues.is_empty()
    })
}

fn attribute(key: &str, value: &Value) -> Option<Value> {
    let value = match value {
        Value::String(v) => json!({"stringValue": v}),
        Value::Bool(v) => json!({"boolValue": v}),
        Value::Number(v) if v.as_i64().is_some() => json!({"intValue": v.to_string()}),
        Value::Null => json!({}),
        _ => return None,
    };
    Some(json!({"key": key, "value": value}))
}
fn version(value: &str) -> bool {
    if value.len() > 32 {
        return false;
    }
    let (base, suffix) = value
        .split_once('-')
        .map_or((value, None), |(a, b)| (a, Some(b)));
    let parts: Vec<_> = base.split('.').collect();
    let number = |v: &str| !v.is_empty() && v.len() <= 5 && v.bytes().all(|b| b.is_ascii_digit());
    parts.len() == 3
        && parts.iter().all(|v| number(v))
        && suffix.is_none_or(|v| {
            let mut parts = v.split('.');
            matches!(parts.next(), Some("alpha" | "beta" | "rc"))
                && parts.next().is_none_or(number)
                && parts.next().is_none()
        })
}
fn allowed(key: &str, value: &Value, policy: &Value, top: bool) -> bool {
    if !policy[if top { "top" } else { "fields" }]
        .as_array()
        .is_some_and(|a| a.iter().any(|v| v == key))
    {
        return false;
    }
    match value {
        Value::String(v) => {
            if top {
                match key {
                    "run_id" | "peer_ref" | "candidate_set_ref" | "connection_id" | "path_ref"
                    | "device_ref" | "address_record_ref" | "capture_id" => {
                        return v.len() == 36 && uuid::Uuid::parse_str(v).is_ok()
                    }
                    "host_version" | "engine_version" => return version(v),
                    "source_commit" => {
                        return v == "unknown"
                            || (v.len() == 40 && v.bytes().all(|b| b.is_ascii_hexdigit()))
                    }
                    _ => {}
                }
            }
            policy["vocabulary"]
                .as_array()
                .is_some_and(|a| a.iter().any(|item| item == v))
        }
        Value::Number(v) => v.as_u64().is_some(),
        Value::Bool(_) | Value::Null => true,
        _ => false,
    }
}
fn log_request(record: Value, policy: &Value) -> Option<Value> {
    if !matches!(record["local_schema_version"].as_u64(), Some(1 | 2)) {
        return None;
    }
    let time = DateTime::parse_from_rfc3339(record["timestamp"].as_str()?)
        .ok()?
        .timestamp_nanos_opt()?;
    if time < 0
        || !matches!(
            record["level"].as_str()?,
            "TRACE" | "DEBUG" | "INFO" | "WARN" | "ERROR"
        )
    {
        return None;
    }
    let mut attributes = Vec::new();
    if let Some(fields) = record["fields"].as_object() {
        for (k, v) in fields {
            if allowed(k, v, policy, false) {
                attributes.extend(attribute(k, v));
            }
        }
    }
    for key in policy["top"].as_array()? {
        let key = key.as_str()?;
        if let Some(value) = record.get(key) {
            if allowed(key, value, policy, true) {
                attributes.extend(attribute(key, value));
            }
        }
    }
    let observed = record["observed_timestamp"]
        .as_str()
        .and_then(|v| DateTime::parse_from_rfc3339(v).ok())
        .and_then(|v| v.timestamp_nanos_opt())
        .filter(|v| *v >= 0)
        .unwrap_or(time);
    let mut log = json!({"timeUnixNano": time.to_string(), "observedTimeUnixNano": observed.to_string(), "severityText": record["level"], "attributes": attributes});
    for (key, output, size) in [("trace_id", "traceId", 32), ("span_id", "spanId", 16)] {
        if let Some(v) = record[key]
            .as_str()
            .filter(|v| v.len() == size && v.bytes().all(|b| b.is_ascii_hexdigit()))
        {
            log[output] = json!(v);
        }
    }
    Some(json!({"resourceLogs": [{"scopeLogs": [{"logRecords": [log]}]}]}))
}

fn run_facts(directory: &Path, policy: &Value) -> Value {
    let read = |name: &str| -> Option<Value> {
        let (file, metadata) =
            crate::network_archive::open_regular_file(&directory.join(name)).ok()?;
        if metadata.len() > 8192 {
            return None;
        }
        serde_json::from_reader(file.take(8192)).ok()
    };
    let run = read("run.json");
    let status = read("status.json");
    let mut facts = json!({"runId": directory.file_name().and_then(|n| n.to_str()), "metadataReadable": run.is_some(), "countsReadable": status.is_some(), "countsComplete": false});
    if let Some(run) = run {
        for key in [
            "schemaVersion",
            "engineSourceCommit",
            "engineSourceState",
            "startedAt",
        ] {
            let value = &run[key];
            let safe = if key == "schemaVersion" {
                value.as_u64() == Some(1)
            } else if key == "engineSourceCommit" {
                value.as_str().is_some_and(|v| {
                    v == "unknown" || (v.len() == 40 && v.bytes().all(|b| b.is_ascii_hexdigit()))
                })
            } else if key == "engineSourceState" {
                value
                    .as_str()
                    .is_some_and(|v| matches!(v, "clean" | "modified" | "unknown"))
            } else {
                value
                    .as_str()
                    .is_some_and(|v| DateTime::parse_from_rfc3339(v).is_ok())
            };
            if safe {
                facts[key] = value.clone();
            }
        }
    }
    if let Some(status) = status {
        for key in policy["statusCounters"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            if status[key].as_u64().is_some() {
                facts[key] = status[key].clone();
            }
        }
        for key in ["closed", "countsComplete"] {
            if status[key].is_boolean() {
                facts[key] = status[key].clone();
            }
        }
        if status["updatedAt"]
            .as_str()
            .is_some_and(|v| DateTime::parse_from_rfc3339(v).is_ok())
        {
            facts["updatedAt"] = status["updatedAt"].clone();
        }
    }
    facts
}

pub(crate) fn write<W: Write + io::Seek>(
    root: &Path,
    since: Option<DateTime<Utc>>,
    zip: &mut ZipWriter<W>,
    options: SimpleFileOptions,
    preparation: Option<&Value>,
    log_limits: &HashMap<PathBuf, u64>,
) -> Result<bool> {
    let policy: Value = serde_json::from_str(POLICY)?;
    let mut outputs = Vec::new();
    let mut sources = Vec::new();
    let mut any = false;
    let mut selection_failed = false;
    let mut runs = Vec::new();
    let mut directories = BTreeSet::new();
    for signal in ["metrics", "logs", "traces"] {
        let name = format!("otel/{signal}.jsonl");
        zip.start_file(&name, options)?;
        let mut hash = Sha256::new();
        let mut bytes = 0_u64;
        let mut requests = 0_u64;
        let (paths, mut issues) = selection(root, signal);
        selection_failed |= !issues.is_empty();
        sources.append(&mut issues);
        for path in paths {
            if let Some(directory) = path.parent().filter(|p| *p != root) {
                if directories.insert(directory.to_path_buf()) {
                    runs.push(run_facts(directory, &policy));
                }
            }
            let label = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .into_owned();
            let (file, metadata) = match crate::network_archive::open_regular_file(&path) {
                Ok(v) => v,
                Err(error) => {
                    sources.push(json!({"name": label, "state": if error.kind() == io::ErrorKind::NotFound {"missing"} else {"unreadable"}}));
                    continue;
                }
            };
            if since.is_some_and(|s| {
                metadata
                    .modified()
                    .is_ok_and(|m| DateTime::<Utc>::from(m) < s)
            }) {
                continue;
            }
            let cutoff = if signal == "logs" {
                log_limits
                    .get(&path)
                    .copied()
                    .unwrap_or(metadata.len())
                    .min(metadata.len())
            } else {
                metadata.len()
            };
            let mut reader = BufReader::with_capacity(8192, file.take(cutoff));
            let mut consumed = 0_u64;
            let mut invalid = 0_u64;
            let mut tail = 0_u64;
            let mut read_failed = false;
            loop {
                // The limited reader caps every allocation, including malicious unterminated lines.
                let mut line = Vec::new();
                let length = match reader
                    .by_ref()
                    .take((LINE_BYTES + 1) as u64)
                    .read_until(b'\n', &mut line)
                {
                    Ok(0) => break,
                    Ok(n) => n,
                    Err(_) => {
                        read_failed = true;
                        break;
                    }
                };
                consumed += length as u64;
                if length > LINE_BYTES {
                    invalid += 1;
                    if line.last() != Some(&b'\n') {
                        loop {
                            let buf = match reader.fill_buf() {
                                Ok(buf) => buf,
                                Err(_) => {
                                    read_failed = true;
                                    break;
                                }
                            };
                            if buf.is_empty() {
                                break;
                            }
                            let n = buf
                                .iter()
                                .position(|b| *b == b'\n')
                                .map_or(buf.len(), |p| p + 1);
                            let ended = buf[n - 1] == b'\n';
                            reader.consume(n);
                            consumed += n as u64;
                            if ended {
                                break;
                            }
                        }
                    }
                    continue;
                }
                if line.last() != Some(&b'\n') {
                    tail += length as u64;
                    break;
                }
                let parsed = serde_json::from_slice::<Value>(&line).ok();
                let value = if signal == "logs" {
                    parsed.and_then(|v| log_request(v, &policy))
                } else {
                    parsed.filter(|v| {
                        v.as_object().is_some_and(|o| {
                            o.len() == 1
                                && o.contains_key(if signal == "metrics" {
                                    "resourceMetrics"
                                } else {
                                    "resourceSpans"
                                })
                        })
                    })
                };
                let Some(value) = value else {
                    invalid += 1;
                    continue;
                };
                let mut output = if signal == "logs" {
                    serde_json::to_vec(&value)?
                } else {
                    line[..line.len() - 1].to_vec()
                };
                output.push(b'\n');
                zip.write_all(&output)?;
                hash.update(&output);
                bytes += output.len() as u64;
                requests += 1;
            }
            sources.push(json!({"name": label, "cutoffBytes": cutoff, "readBytes": consumed, "truncated": consumed < cutoff, "readFailed": read_failed, "invalidLines": invalid, "tailBytes": tail, "observedAt": Utc::now().to_rfc3339()}));
        }
        any |= requests > 0;
        outputs.push(json!({"name": name, "bytes": bytes, "requests": requests, "sha256": format!("{:x}",hash.finalize()), "state": if requests>0 {"partial"} else {"missing"}}));
    }
    let summary = json!({"schemaVersion":1, "format":"OTLP JSONL", "collectorVersion":"0.139.0", "liveFlush":preparation.is_some(), "recording":preparation.and_then(|v| v.get("performance")), "state":if any {"partial"}else{"unavailable"}, "reason":"file_prefixes_other_processes_not_flushed", "countsComplete":false, "files":outputs, "sources":sources, "runs":runs});
    zip.start_file("otel/manifest.json", options)?;
    serde_json::to_writer_pretty(&mut *zip, &summary)?;
    for (name, content) in [
        (
            "import/import.py",
            include_str!("../../../packages/desktop-host-go/diaglogs/import/import.py"),
        ),
        (
            "import/collector.yaml",
            include_str!("../../../packages/desktop-host-go/diaglogs/import/collector.yaml"),
        ),
        (
            "import/README.md",
            include_str!("../../../packages/desktop-host-go/diaglogs/import/README.md"),
        ),
    ] {
        zip.start_file(name, options)?;
        zip.write_all(content.as_bytes())?;
    }
    Ok(any || selection_failed)
}
