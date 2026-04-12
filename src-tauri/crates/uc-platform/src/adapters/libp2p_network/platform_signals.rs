//! Platform signal listeners for recovery triggers.
//!
//! Emits two kinds of [`PlatformSignal`] into a single channel consumed by
//! `run_swarm`:
//!
//! - `NetworkChange` — the physical LAN IPv4 address changed (all platforms,
//!   via polling [`get_physical_lan_ip`])
//! - `SleepWake`    — the device just resumed from sleep (macOS via IOKit)
//!
//! Non-macOS platforms currently receive only network-change signals. Linux
//! (D-Bus / logind) and Windows (WM_POWERBROADCAST) sleep/wake integration is
//! deferred; the recovery coordinator still has reactive triggers (mDNS expiry,
//! dial-failure streak, first-attempt-after-idle) that keep Wave 1 functional
//! without platform notifications.

use std::net::Ipv4Addr;
use std::time::Duration;

use tokio::sync::mpsc;
use tracing::{debug, info};

use super::swarm_event_loop::PlatformSignal;
use crate::net_utils::get_physical_lan_ip;

/// Bounded buffer for platform signals. Signals are rare and the swarm loop
/// drains them promptly, so a small capacity is sufficient and bounds memory
/// if the consumer ever stalls.
const PLATFORM_SIGNAL_CHANNEL_CAPACITY: usize = 16;

/// Polling cadence for the network-change listener. 3s is fast enough to
/// trigger recovery well inside the 120s window and slow enough to be
/// negligible.
const NETWORK_POLL_INTERVAL: Duration = Duration::from_secs(3);

/// Spawn all available platform signal listeners and return the receiver.
///
/// Exactly one call per `spawn_swarm` invocation. Listeners live for the
/// remainder of the process; dropping the receiver simply turns subsequent
/// `Sender::send` calls into no-ops because the polling task will notice the
/// closed channel and exit.
pub(super) fn spawn_platform_signal_listener() -> mpsc::Receiver<PlatformSignal> {
    let (tx, rx) = mpsc::channel(PLATFORM_SIGNAL_CHANNEL_CAPACITY);

    spawn_network_change_listener(tx.clone());

    #[cfg(all(target_os = "macos", not(test)))]
    macos::spawn_sleep_wake_listener(tx);

    #[cfg(not(all(target_os = "macos", not(test))))]
    {
        // Suppress unused warning on platforms without a sleep/wake listener.
        let _ = tx;
        info!(
            event = "platform.sleep_wake_listener_unavailable",
            "sleep/wake listener not active on this build; recovery will fall back to reactive triggers (mDNS expiry, dial failures, first-attempt-after-idle)"
        );
    }

    rx
}

/// Poll the physical LAN IPv4 at [`NETWORK_POLL_INTERVAL`] and emit
/// `NetworkChange` whenever the observed address differs from the previous
/// sample.
fn spawn_network_change_listener(tx: mpsc::Sender<PlatformSignal>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(NETWORK_POLL_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        // The first tick fires immediately; consume it so the *next* tick is
        // one interval from now.
        interval.tick().await;

        let mut last_ip: Option<Ipv4Addr> = get_physical_lan_ip();
        info!(
            event = "platform.network_change_listener_started",
            initial_ip = ?last_ip,
            poll_interval_secs = NETWORK_POLL_INTERVAL.as_secs(),
            "network-change listener started"
        );

        loop {
            interval.tick().await;
            let current = get_physical_lan_ip();
            if current != last_ip {
                info!(
                    event = "platform.network_change_detected",
                    previous_ip = ?last_ip,
                    current_ip = ?current,
                    "local LAN IPv4 changed"
                );
                last_ip = current;
                if tx.send(PlatformSignal::NetworkChange).await.is_err() {
                    debug!(
                        event = "platform.network_change_listener_exit",
                        "platform signal receiver dropped; network-change listener exiting"
                    );
                    return;
                }
            }
        }
    });
}

#[cfg(all(target_os = "macos", not(test)))]
mod macos {
    //! macOS sleep/wake listener via IOKit.
    //!
    //! Registers with `IORegisterForSystemPower` on a dedicated std::thread
    //! that runs a `CFRunLoop`. The power callback acknowledges
    //! `CanSystemSleep` / `SystemWillSleep` immediately so we don't delay
    //! system sleep, and forwards `SystemHasPoweredOn` into the signal channel.

    use std::ffi::c_void;
    use std::ptr;
    use std::sync::atomic::{AtomicU32, Ordering};

    use libc::{c_int, c_uint};
    use tokio::sync::mpsc;
    use tracing::{info, warn};

    use super::PlatformSignal;

    // ── IOKit / CoreFoundation opaque handle types ────────────────────────
    type IONotificationPortRef = *mut c_void;
    #[allow(non_camel_case_types)]
    type io_object_t = c_uint;
    #[allow(non_camel_case_types)]
    type io_connect_t = io_object_t;
    #[allow(non_camel_case_types)]
    type io_service_t = io_object_t;
    type CFRunLoopRef = *mut c_void;
    type CFRunLoopSourceRef = *mut c_void;
    type CFStringRef = *const c_void;

    type IOServiceInterestCallback = extern "C" fn(
        refcon: *mut c_void,
        service: io_service_t,
        message_type: u32,
        message_argument: *mut c_void,
    );

    // IOMessage.h — IOKit power-management notification message types.
    const K_IO_MESSAGE_CAN_SYSTEM_SLEEP: u32 = 0xe000_0270;
    const K_IO_MESSAGE_SYSTEM_WILL_SLEEP: u32 = 0xe000_0280;
    const K_IO_MESSAGE_SYSTEM_HAS_POWERED_ON: u32 = 0xe000_0300;

    #[link(name = "IOKit", kind = "framework")]
    extern "C" {
        fn IORegisterForSystemPower(
            refcon: *mut c_void,
            the_port_ref: *mut IONotificationPortRef,
            callback: IOServiceInterestCallback,
            notifier: *mut io_object_t,
        ) -> io_connect_t;
        fn IONotificationPortGetRunLoopSource(notify: IONotificationPortRef) -> CFRunLoopSourceRef;
        fn IOAllowPowerChange(kernel_port: io_connect_t, notification_id: isize) -> c_int;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRunLoopGetCurrent() -> CFRunLoopRef;
        fn CFRunLoopAddSource(rl: CFRunLoopRef, source: CFRunLoopSourceRef, mode: CFStringRef);
        fn CFRunLoopRun();
        static kCFRunLoopDefaultMode: CFStringRef;
    }

    /// Callback state stored behind `Box::into_raw` and passed to IOKit as
    /// the `refcon`. `root_port` is filled in after `IORegisterForSystemPower`
    /// returns and read atomically by the callback.
    struct PowerContext {
        tx: mpsc::Sender<PlatformSignal>,
        root_port: AtomicU32,
    }

    extern "C" fn power_callback(
        refcon: *mut c_void,
        _service: io_service_t,
        message_type: u32,
        message_argument: *mut c_void,
    ) {
        // Safety: `refcon` was created via `Box::into_raw` and remains valid
        // for the lifetime of the listener thread (which is the only thread
        // invoking this callback).
        let ctx = unsafe { &*(refcon as *const PowerContext) };

        match message_type {
            K_IO_MESSAGE_CAN_SYSTEM_SLEEP | K_IO_MESSAGE_SYSTEM_WILL_SLEEP => {
                // Acknowledge immediately so we do not delay system sleep.
                // Failing to respond causes a ~30s sleep-delay timeout.
                let root_port = ctx.root_port.load(Ordering::Acquire);
                unsafe {
                    IOAllowPowerChange(root_port, message_argument as isize);
                }
            }
            K_IO_MESSAGE_SYSTEM_HAS_POWERED_ON => {
                info!(
                    event = "platform.sleep_wake_detected",
                    "system woke from sleep"
                );
                match ctx.tx.try_send(PlatformSignal::SleepWake) {
                    Ok(()) => {}
                    Err(mpsc::error::TrySendError::Full(_)) => {
                        warn!(
                            event = "platform.sleep_wake_signal_dropped",
                            reason = "channel_full",
                            "sleep/wake signal dropped; channel full — swarm has queued signals"
                        );
                    }
                    Err(mpsc::error::TrySendError::Closed(_)) => {
                        info!(
                            event = "platform.sleep_wake_signal_dropped",
                            reason = "receiver_closed",
                            "sleep/wake signal dropped; receiver closed"
                        );
                    }
                }
            }
            _ => {}
        }
    }

    pub(super) fn spawn_sleep_wake_listener(tx: mpsc::Sender<PlatformSignal>) {
        let result = std::thread::Builder::new()
            .name("platform-sleep-wake".into())
            .spawn(move || unsafe { run_listener(tx) });

        if let Err(err) = result {
            warn!(
                event = "platform.sleep_wake_listener_spawn_failed",
                error = %err,
                "failed to spawn macOS sleep/wake listener thread"
            );
        }
    }

    unsafe fn run_listener(tx: mpsc::Sender<PlatformSignal>) {
        let ctx = Box::into_raw(Box::new(PowerContext {
            tx,
            root_port: AtomicU32::new(0),
        }));

        let mut port: IONotificationPortRef = ptr::null_mut();
        let mut notifier: io_object_t = 0;

        let root_port =
            IORegisterForSystemPower(ctx as *mut c_void, &mut port, power_callback, &mut notifier);

        if root_port == 0 || port.is_null() {
            warn!(
                event = "platform.sleep_wake_listener_failed",
                "IORegisterForSystemPower failed; macOS sleep/wake detection disabled"
            );
            drop(Box::from_raw(ctx));
            return;
        }

        (*ctx).root_port.store(root_port, Ordering::Release);

        let source = IONotificationPortGetRunLoopSource(port);
        let rl = CFRunLoopGetCurrent();
        CFRunLoopAddSource(rl, source, kCFRunLoopDefaultMode);

        info!(
            event = "platform.sleep_wake_listener_started",
            "macOS sleep/wake listener started"
        );

        // Blocks this thread forever. The process exit tears the thread down.
        CFRunLoopRun();

        // Reached only if CFRunLoopRun ever returns (it normally does not).
        drop(Box::from_raw(ctx));
    }
}
