//! X11 剪贴板 watcher 集成测试(issue #1029 回归)。
//!
//! 这些测试需要一个可用的 X display,因此全部 `#[ignore]`。运行方式:
//!
//! ```bash
//! xvfb-run -a cargo test -p uc-platform --test x11_watcher -- --ignored --test-threads=1
//! ```
//!
//! 必须 `--test-threads=1`:所有用例共享同一个 X server 的 `CLIPBOARD`
//! selection,并发运行会互相抢所有权。
//!
//! 测试通过一个"可编程假 owner"(直接用 x11rb 实现的 selection owner,
//! 可配置响应延迟 / 拒绝窗口)模拟 Chromium 经 XWayland 桥时的两种坏行为:
//!
//! 1. watcher 读取期间 selection 再次易主 —— 修复前该变更事件会在
//!    `route_unrelated_event` 中被吞掉,内容永久丢失;
//! 2. owner 在拿到所有权后的短窗口内拒绝供数 —— 修复前单次空读即静默放弃。

#![cfg(target_os = "linux")]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use uc_platform::clipboard::watcher::{ClipboardWatcher, PlatformEvent};
use uc_platform::clipboard::{build_event_loop, shutdown_channel, NoopSystemClipboard, ShutdownTx};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    Atom, AtomEnum, ConnectionExt as _, CreateWindowAux, EventMask, PropMode, SelectionNotifyEvent,
    WindowClass, SELECTION_NOTIFY_EVENT,
};
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

/// 单个事件的最长等待时间。涵盖最坏情况:2s TARGETS 超时 + 3 次重试。
const EVENT_DEADLINE: Duration = Duration::from_secs(8);

// ---------------------------------------------------------------------------
// 可编程假 selection owner
// ---------------------------------------------------------------------------

/// 假 owner 的行为脚本。
#[derive(Clone, Copy)]
struct OwnerSpec {
    /// 对 `UTF8_STRING` 请求返回的内容。
    text: &'static [u8],
    /// 响应 `UTF8_STRING` 数据请求前的人为延迟 —— 把 watcher 钉在读取中,
    /// 制造"读取期间 selection 易主"的竞态窗口。注意延迟必须放在数据
    /// 响应而不是 TARGETS 上:`convert_selection` 永远发给当前 owner,
    /// 若 TARGETS 之后才易主,同一次读取会顺带从新 owner 拿到数据,
    /// 旧代码也能侥幸通过。
    data_delay: Duration,
    /// 拿到所有权后这段时间内拒绝一切请求(回 property=NONE),
    /// 模拟 Chromium 复制后短暂无法供数的窗口。
    refuse_for: Duration,
}

impl OwnerSpec {
    fn well_behaved(text: &'static [u8]) -> Self {
        Self {
            text,
            data_delay: Duration::ZERO,
            refuse_for: Duration::ZERO,
        }
    }
}

struct FakeOwner {
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl FakeOwner {
    /// 启动假 owner 并阻塞到它确实拿到 `CLIPBOARD` 所有权后返回。
    fn spawn(spec: OwnerSpec) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let join = std::thread::spawn(move || owner_main(spec, thread_stop, ready_tx));
        ready_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("fake owner failed to acquire CLIPBOARD ownership");
        Self {
            stop,
            join: Some(join),
        }
    }
}

impl Drop for FakeOwner {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

fn intern(conn: &RustConnection, name: &[u8]) -> Atom {
    conn.intern_atom(false, name)
        .expect("intern_atom request")
        .reply()
        .expect("intern_atom reply")
        .atom
}

fn owner_main(spec: OwnerSpec, stop: Arc<AtomicBool>, ready_tx: std::sync::mpsc::Sender<()>) {
    let (conn, screen_num) = x11rb::connect(None).expect("fake owner: X connect");
    let screen = &conn.setup().roots[screen_num];
    let win = conn.generate_id().expect("fake owner: generate_id");
    conn.create_window(
        x11rb::COPY_DEPTH_FROM_PARENT,
        win,
        screen.root,
        0,
        0,
        1,
        1,
        0,
        WindowClass::INPUT_OUTPUT,
        screen.root_visual,
        &CreateWindowAux::new(),
    )
    .expect("fake owner: create_window request")
    .check()
    .expect("fake owner: create_window");

    let clipboard = intern(&conn, b"CLIPBOARD");
    let targets = intern(&conn, b"TARGETS");
    let utf8_string = intern(&conn, b"UTF8_STRING");

    conn.set_selection_owner(win, clipboard, x11rb::CURRENT_TIME)
        .expect("fake owner: set_selection_owner request")
        .check()
        .expect("fake owner: set_selection_owner");
    conn.flush().expect("fake owner: flush");
    let owned_at = Instant::now();
    let _ = ready_tx.send(());

    while !stop.load(Ordering::Relaxed) {
        let event = conn.poll_for_event().expect("fake owner: poll_for_event");
        let Some(event) = event else {
            std::thread::sleep(Duration::from_millis(5));
            continue;
        };
        let Event::SelectionRequest(req) = event else {
            // SelectionClear(所有权被抢走)等事件:继续服务已收到的
            // 请求即可,与真实的慢 owner 行为一致。
            continue;
        };

        let mut reply_property = req.property;
        if owned_at.elapsed() < spec.refuse_for {
            reply_property = x11rb::NONE;
        } else if req.target == targets {
            conn.change_property32(
                PropMode::REPLACE,
                req.requestor,
                req.property,
                AtomEnum::ATOM,
                &[targets, utf8_string],
            )
            .expect("fake owner: change_property (TARGETS)")
            .check()
            .expect("fake owner: change_property check (TARGETS)");
        } else if req.target == utf8_string {
            std::thread::sleep(spec.data_delay);
            conn.change_property8(
                PropMode::REPLACE,
                req.requestor,
                req.property,
                utf8_string,
                spec.text,
            )
            .expect("fake owner: change_property (UTF8_STRING)")
            .check()
            .expect("fake owner: change_property check (UTF8_STRING)");
        } else {
            reply_property = x11rb::NONE;
        }

        let notify = SelectionNotifyEvent {
            response_type: SELECTION_NOTIFY_EVENT,
            sequence: 0,
            time: req.time,
            requestor: req.requestor,
            selection: req.selection,
            target: req.target,
            property: reply_property,
        };
        conn.send_event(false, req.requestor, EventMask::NO_EVENT, notify)
            .expect("fake owner: send_event request")
            .check()
            .expect("fake owner: send_event");
        conn.flush().expect("fake owner: flush after reply");
    }
}

// ---------------------------------------------------------------------------
// watcher 测试夹具
// ---------------------------------------------------------------------------

struct WatcherHarness {
    rx: tokio::sync::mpsc::Receiver<PlatformEvent>,
    shutdown: ShutdownTx,
    join: Option<JoinHandle<anyhow::Result<()>>>,
}

impl WatcherHarness {
    fn start() -> Self {
        // 强制选择 X11 后端:开发机的 Wayland 会话里 WAYLAND_DISPLAY 会让
        // build_event_loop 优先选 wayland。进程级 env 修改没问题 ——
        // 本文件要求 --test-threads=1 串行执行。
        std::env::remove_var("WAYLAND_DISPLAY");

        let (tx, rx) = tokio::sync::mpsc::channel(16);
        let watcher = ClipboardWatcher::new(Arc::new(NoopSystemClipboard), tx);
        let event_loop = build_event_loop().expect("X display required (run under xvfb-run)");
        let (shutdown, shutdown_rx) = shutdown_channel();
        let join = std::thread::spawn(move || event_loop.run(watcher, shutdown_rx));
        Self {
            rx,
            shutdown,
            join: Some(join),
        }
    }

    /// 在 deadline 内等待一条文本内容恰为 `expected` 的剪贴板事件。
    /// 其间允许出现其他事件(例如竞态用例里第一个 owner 的内容)。
    async fn expect_text_event(&mut self, expected: &[u8]) {
        let deadline = Instant::now() + EVENT_DEADLINE;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            assert!(
                !remaining.is_zero(),
                "timed out waiting for clipboard event with text {:?}",
                String::from_utf8_lossy(expected)
            );
            let event = tokio::time::timeout(remaining, self.rx.recv())
                .await
                .unwrap_or_else(|_| {
                    panic!(
                        "timed out waiting for clipboard event with text {:?}",
                        String::from_utf8_lossy(expected)
                    )
                })
                .expect("watcher channel closed unexpectedly");
            let PlatformEvent::ClipboardChanged { snapshot } = event;
            let matched = snapshot
                .representations
                .iter()
                .filter_map(|rep| rep.inline_bytes())
                .any(|bytes| bytes == expected);
            if matched {
                return;
            }
        }
    }
}

impl Drop for WatcherHarness {
    fn drop(&mut self) {
        self.shutdown.signal();
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

// ---------------------------------------------------------------------------
// 用例
// ---------------------------------------------------------------------------

/// 基线:行为良好的 owner,一次复制一次捕获。
#[tokio::test]
#[ignore = "requires an X display (xvfb-run)"]
async fn captures_basic_copy() {
    let mut harness = WatcherHarness::start();
    let _owner = FakeOwner::spawn(OwnerSpec::well_behaved(b"x11-watcher-basic"));
    harness.expect_text_event(b"x11-watcher-basic").await;
}

/// 竞态回归(issue #1029):owner A 故意拖慢数据响应,把 watcher 钉在
/// 读取中;此时 owner B 抢走 selection。B 的 XfixesSelectionNotify 会在
/// 读取期间被消费 —— 修复前直接被丢弃,第一次读取以 A 的旧内容收尾,
/// B 的内容永久丢失;修复后通过 pending 标志补读,最终必须捕获到 B 的内容。
#[tokio::test]
#[ignore = "requires an X display (xvfb-run)"]
async fn change_during_read_is_not_lost() {
    let mut harness = WatcherHarness::start();

    let _owner_a = FakeOwner::spawn(OwnerSpec {
        text: b"x11-watcher-race-old",
        data_delay: Duration::from_millis(400),
        refuse_for: Duration::ZERO,
    });
    // 给 watcher 时间收到 A 的变更通知并进入(被拖慢的)读取。
    tokio::time::sleep(Duration::from_millis(100)).await;
    let _owner_b = FakeOwner::spawn(OwnerSpec::well_behaved(b"x11-watcher-race-new"));

    harness.expect_text_event(b"x11-watcher-race-new").await;
}

/// 重试回归(issue #1029):owner 在拿到所有权后的前 180ms 拒绝一切请求
/// (Chromium 经 XWayland 桥的典型坏窗口)。修复前首次空读即静默放弃;
/// 修复后 150ms 间隔的重试应拿到内容。
#[tokio::test]
#[ignore = "requires an X display (xvfb-run)"]
async fn retry_recovers_initially_refusing_owner() {
    let mut harness = WatcherHarness::start();

    let _owner = FakeOwner::spawn(OwnerSpec {
        text: b"x11-watcher-retry",
        data_delay: Duration::ZERO,
        refuse_for: Duration::from_millis(180),
    });

    harness.expect_text_event(b"x11-watcher-retry").await;
}
