#![cfg(feature = "dev-tools")]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

struct CliProfile {
    name: String,
    data_dir: PathBuf,
    cache_dir: PathBuf,
    log_dir: PathBuf,
}

impl CliProfile {
    fn new(label: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let name = format!("cli-dir-{label}-{}-{nonce}", std::process::id());
        Self {
            data_dir: profile_data_dir(&name),
            cache_dir: profile_cache_dir(&name),
            log_dir: profile_log_dir(&name),
            name,
        }
    }

    fn run(&self, args: &[String]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_uniclip"))
            .args(["--dev", "--profile", self.name.as_str()])
            .args(args)
            .output()
            .expect("run uniclip")
    }

    fn init(&self) {
        let output = self.run(&[
            "init".to_string(),
            "--passphrase".to_string(),
            "directory-e2e-passphrase".to_string(),
            "--device-name".to_string(),
            "directory-e2e".to_string(),
        ]);
        assert_success(&output, "init");

        let stop = self.run(&["stop".to_string()]);
        assert_success(&stop, "stop transient daemon");
    }

    fn capture(&self, paths: &[&Path], extra: &[&str]) -> Value {
        let mut args = vec![
            "--json".to_string(),
            "dev".to_string(),
            "capture-files".to_string(),
        ];
        for path in paths {
            args.push("--path".to_string());
            args.push(path.to_string_lossy().into_owned());
        }
        args.extend(extra.iter().map(|value| (*value).to_string()));
        let output = self.run(&args);
        assert_success(&output, "capture-files");
        serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
            panic!(
                "capture-files returned invalid JSON: {error}; stdout={}; stderr={}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
        })
    }
}

impl Drop for CliProfile {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.data_dir);
        let _ = std::fs::remove_dir_all(&self.cache_dir);
        let _ = std::fs::remove_dir_all(&self.log_dir);
    }
}

fn assert_success(output: &Output, action: &str) {
    assert!(
        output.status.success(),
        "{action} failed: exit={:?}; stdout={}; stderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn member_pairs(value: &Value) -> Vec<(&str, &str)> {
    value["lines"]
        .as_array()
        .expect("lines array")
        .iter()
        .filter_map(|line| Some((line["relativePath"].as_str()?, line["memberKind"].as_str()?)))
        .collect()
}

#[test]
#[ignore = "requires a prebuilt uniclipd sibling"]
fn cli_captures_directory_members_and_deduplicates_repeat_capture() {
    let profile = CliProfile::new("tree");
    profile.init();
    let fixture = tempfile::tempdir().expect("fixture dir");
    let root = fixture.path().join("root");
    std::fs::create_dir_all(root.join("nested")).unwrap();
    std::fs::create_dir(root.join("empty")).unwrap();
    std::fs::write(root.join("visible.txt"), b"visible").unwrap();
    std::fs::write(root.join(".hidden"), b"hidden").unwrap();
    std::fs::write(root.join("e\u{301}.txt"), b"unicode").unwrap();
    let executable = root.join("nested/run.sh");
    std::fs::write(&executable, b"#!/bin/sh\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    let first = profile.capture(&[&root], &[]);
    assert!(!first
        .to_string()
        .contains(&root.to_string_lossy().to_string()));
    assert_eq!(first["directoryStructure"], true);
    assert_eq!(first["contentDigestCount"], 0);
    assert_eq!(first["deduplicated"], false);
    assert!(first["lines"]
        .as_array()
        .expect("lines array")
        .iter()
        .all(|line| line["rootName"] == "root"));
    let members = member_pairs(&first);
    assert!(members.contains(&(".hidden", "f")));
    assert!(members.contains(&("visible.txt", "f")));
    assert!(members.contains(&("empty", "d")));
    assert!(members.contains(&("\u{e9}.txt", "f")));
    #[cfg(unix)]
    assert!(members.contains(&("nested/run.sh", "x")));

    let second = profile.capture(&[&root], &[]);
    assert_eq!(second["deduplicated"], true);
    assert_eq!(second["entryId"], first["entryId"]);
}

#[test]
#[ignore = "requires a prebuilt uniclipd sibling"]
fn cli_captures_mixed_roots_and_applies_member_cap() {
    let profile = CliProfile::new("mixed-cap");
    profile.init();
    let fixture = tempfile::tempdir().expect("fixture dir");
    let loose = fixture.path().join("loose.txt");
    std::fs::write(&loose, b"loose").unwrap();
    let root = fixture.path().join("root");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("child.txt"), b"child").unwrap();

    let mixed = profile.capture(&[&loose, &root], &[]);
    let lines = mixed["lines"].as_array().expect("lines array");
    assert!(lines
        .iter()
        .any(|line| { line["rootIndex"] == 0 && line["relativePath"] == "loose.txt" }));
    assert!(lines
        .iter()
        .any(|line| { line["rootIndex"] == 1 && line["relativePath"] == "child.txt" }));

    let capped_root = fixture.path().join("capped");
    std::fs::create_dir(&capped_root).unwrap();
    for name in ["a", "b", "c"] {
        std::fs::write(capped_root.join(name), name.as_bytes()).unwrap();
    }
    let capped = profile.capture(&[&capped_root], &["--max-members", "2"]);
    assert!(capped["lines"]
        .as_array()
        .expect("lines array")
        .iter()
        .all(|line| {
            line["lineKind"] == "excluded" && line["excludeReason"] == "size_cap_exceeded"
        }));

    let byte_capped = fixture.path().join("byte-capped");
    std::fs::create_dir(&byte_capped).unwrap();
    std::fs::write(byte_capped.join("large"), b"four").unwrap();
    let byte_result = profile.capture(&[&byte_capped], &["--max-bytes", "1"]);
    assert!(byte_result["lines"]
        .as_array()
        .expect("lines array")
        .iter()
        .all(|line| line["excludeReason"] == "size_cap_exceeded"));

    let after_override = fixture.path().join("after-override.txt");
    std::fs::write(&after_override, b"normal").unwrap();
    let restored = profile.capture(&[&after_override], &[]);
    assert_eq!(restored["lines"][0]["lineKind"], "file");
    assert_eq!(restored["contentDigestCount"], 1);
}

#[cfg(unix)]
#[test]
#[ignore = "requires a prebuilt uniclipd sibling"]
fn cli_rejects_symlink_and_socket_members() {
    use std::os::unix::fs::symlink;
    use std::os::unix::net::UnixListener;

    let profile = CliProfile::new("unsupported");
    profile.init();
    for kind in ["symlink", "socket"] {
        let fixture = tempfile::tempdir().expect("fixture dir");
        let root = fixture.path().join("root");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("target.txt"), b"target").unwrap();
        let _socket = if kind == "symlink" {
            symlink(root.join("target.txt"), root.join("forbidden")).unwrap();
            None
        } else {
            Some(UnixListener::bind(root.join("forbidden")).unwrap())
        };

        let result = profile.capture(&[&root], &[]);
        assert!(result["lines"]
            .as_array()
            .expect("lines array")
            .iter()
            .any(|line| line["excludeReason"] == "unsupported_member"));
    }
}

fn profile_data_dir(profile: &str) -> PathBuf {
    #[cfg(target_os = "macos")]
    return home_dir()
        .join("Library/Application Support")
        .join(format!("app.uniclipboard.desktop-{profile}"));
    #[cfg(target_os = "linux")]
    return std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().join(".local/share"))
        .join(format!("app.uniclipboard.desktop-{profile}"));
    #[cfg(target_os = "windows")]
    return std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Temp"))
        .join(format!("app.uniclipboard.desktop-{profile}"));
}

fn profile_cache_dir(profile: &str) -> PathBuf {
    #[cfg(target_os = "macos")]
    return home_dir()
        .join("Library/Caches")
        .join(format!("app.uniclipboard.desktop-{profile}"));
    #[cfg(target_os = "linux")]
    return std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().join(".cache"))
        .join(format!("app.uniclipboard.desktop-{profile}"));
    #[cfg(target_os = "windows")]
    return profile_data_dir(profile).join("cache");
}

fn profile_log_dir(profile: &str) -> PathBuf {
    #[cfg(target_os = "macos")]
    return home_dir()
        .join("Library/Logs")
        .join(format!("app.uniclipboard.desktop-{profile}"));
    #[cfg(target_os = "linux")]
    return std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().join(".local/state"))
        .join(format!("app.uniclipboard.desktop-{profile}"));
    #[cfg(target_os = "windows")]
    return std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Temp"))
        .join(format!("app.uniclipboard.desktop-{profile}"))
        .join("logs");
}

fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .expect("HOME is set")
}
