#![cfg(target_os = "macos")]

use objc2::rc::autoreleasepool;
use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString};
use std::process::Command;
use uc_platform::clipboard::platform::macos::MacOSClipboard;
use uc_platform::clipboard::{
    MimeType, ObservedClipboardRepresentation, RepresentationId, SystemClipboard,
    SystemClipboardSnapshot,
};

const CHILD_ENV: &str = "UC_TEST_CLIPBOARD_AUTORELEASE_CHILD";
const TEST_NAME: &str = "clipboard_operations_bound_native_memory";
const MIB: usize = 1024 * 1024;

#[repr(C)]
#[derive(Default)]
struct MallocStatistics {
    blocks_in_use: u32,
    size_in_use: usize,
    max_size_in_use: usize,
    size_allocated: usize,
}

fn allocated_bytes() -> usize {
    extern "C" {
        fn malloc_zone_statistics(zone: *mut std::ffi::c_void, stats: *mut MallocStatistics);
    }
    let mut stats = MallocStatistics::default();
    // SAFETY: A null zone requests totals across all zones; stats matches malloc_statistics_t.
    unsafe { malloc_zone_statistics(std::ptr::null_mut(), &mut stats) };
    stats.size_in_use
}

#[test]
fn clipboard_operations_bound_native_memory() {
    if std::env::var_os(CHILD_ENV).is_none() {
        let temporary = tempfile::tempdir().unwrap();
        let library = temporary.path().join("private-pasteboard.dylib");
        let writer = temporary.path().join("private-pasteboard");
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../scripts/e2e/clipboard-startup/private-pasteboard.m");
        let compile = Command::new("clang")
            .args(["-fobjc-arc", "-dynamiclib", "-framework", "AppKit"])
            .arg(&fixture)
            .arg("-o")
            .arg(&library)
            .output()
            .unwrap();
        assert!(
            compile.status.success(),
            "private pasteboard fixture failed: {}",
            String::from_utf8_lossy(&compile.stderr)
        );
        let compile = Command::new("clang")
            .args([
                "-fobjc-arc",
                "-framework",
                "AppKit",
                "-DPRIVATE_PASTEBOARD_TOOL",
            ])
            .arg(&fixture)
            .arg("-o")
            .arg(&writer)
            .output()
            .unwrap();
        assert!(
            compile.status.success(),
            "private pasteboard writer failed: {}",
            String::from_utf8_lossy(&compile.stderr)
        );
        let board_name = format!("org.uniclipboard.e2e.autorelease.{}", std::process::id());
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", TEST_NAME, "--nocapture"])
            .env(CHILD_ENV, "1")
            .env("UC_TEST_CLIPBOARD_WRITER", &writer)
            .env("DYLD_INSERT_LIBRARIES", &library)
            .env("UC_E2E_PRIVATE_PASTEBOARD", &board_name)
            .output()
            .unwrap();
        let cleanup = Command::new(&writer)
            .arg("release")
            .env("UC_E2E_PRIVATE_PASTEBOARD", &board_name)
            .env_remove("DYLD_INSERT_LIBRARIES")
            .output()
            .unwrap();
        assert!(
            cleanup.status.success(),
            "private pasteboard cleanup failed"
        );
        assert!(
            output.status.success(),
            "isolated clipboard regression failed:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        eprint!("{}", String::from_utf8_lossy(&output.stderr));
        return;
    }

    extern "C" {
        fn NSApplicationLoad() -> bool;
    }
    // SAFETY: Bootstrap AppKit in this isolated test process, as the daemon does.
    assert!(unsafe { NSApplicationLoad() });
    let clipboard = MacOSClipboard::new().unwrap();
    let pasteboard = NSPasteboard::generalPasteboard();
    assert!(pasteboard
        .name()
        .to_string()
        .starts_with("org.uniclipboard.e2e."));
    let temporary = tempfile::tempdir().unwrap();
    let image_path = temporary.path().join("fixture.png");
    let mut state = 1_u32;
    let pixels = (0..768 * 768 * 3)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state as u8
        })
        .collect();
    image::RgbImage::from_raw(768, 768, pixels)
        .unwrap()
        .save(&image_path)
        .unwrap();
    let payload = std::fs::read(&image_path).unwrap();
    let writer = std::env::var_os("UC_TEST_CLIPBOARD_WRITER").unwrap();

    // Warm the native services before measuring. The caller deliberately has no
    // per-operation pool: a long-lived watcher relies on the adapter to drain it.
    let mut baseline = 0;
    for iteration in 0..36 {
        // A separate owner forces an IPC read instead of reusing our own NSData.
        let written = Command::new(&writer)
            .arg("image")
            .arg(&image_path)
            .env_remove("DYLD_INSERT_LIBRARIES")
            .output()
            .unwrap();
        assert!(written.status.success(), "private image write failed");
        let (snapshot, identity) = clipboard.read_observation().unwrap();
        assert_eq!(identity.unwrap(), pasteboard.changeCount().to_le_bytes());
        assert!(snapshot
            .representations
            .iter()
            .any(|rep| rep.inline_bytes() == Some(payload.as_slice())));
        drop(snapshot);

        let text = format!("autorelease fixture {iteration}").into_bytes();
        let snapshot = SystemClipboardSnapshot {
            ts_ms: 1,
            representations: vec![ObservedClipboardRepresentation::new(
                RepresentationId::new(),
                "text".into(),
                Some(MimeType::text_plain()),
                text.clone(),
            )],
            file_content_digests: vec![],
            file_set_v1_component: None,
        };
        let receipt = clipboard.write_with_receipt(snapshot).unwrap().unwrap();
        assert_eq!(receipt, pasteboard.changeCount().to_le_bytes());
        autoreleasepool(|_| {
            assert_eq!(
                pasteboard
                    .dataForType(unsafe { NSPasteboardTypeString })
                    .unwrap()
                    .to_vec(),
                text
            );
        });
        if iteration == 3 {
            baseline = allocated_bytes();
        }
    }
    let growth = allocated_bytes().saturating_sub(baseline);
    autoreleasepool(|_| pasteboard.clearContents());
    eprintln!("native allocation growth over 32 clipboard operations: {growth} bytes");
    assert!(
        growth < 16 * MIB,
        "native clipboard allocations accumulated: {growth} bytes"
    );
}
