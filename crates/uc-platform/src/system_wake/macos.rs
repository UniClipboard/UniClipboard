use super::*;
use std::ffi::c_void;
use std::ptr;

type Ref = *mut c_void;
const CAN_SLEEP: u32 = 0xe0000270;
const WILL_SLEEP: u32 = 0xe0000280;
const DID_WAKE: u32 = 0xe0000300;

#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IORegisterForSystemPower(
        context: Ref,
        port: *mut Ref,
        callback: unsafe extern "C" fn(Ref, u32, u32, Ref),
        notifier: *mut u32,
    ) -> u32;
    fn IODeregisterForSystemPower(notifier: *mut u32) -> i32;
    fn IONotificationPortGetRunLoopSource(port: Ref) -> Ref;
    fn IONotificationPortDestroy(port: Ref);
    fn IOServiceClose(connection: u32) -> i32;
    fn IOAllowPowerChange(connection: u32, notification: isize) -> i32;
}
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    static kCFRunLoopDefaultMode: Ref;
    fn CFRunLoopGetCurrent() -> Ref;
    fn CFRunLoopAddSource(run_loop: Ref, source: Ref, mode: Ref);
    fn CFRunLoopRemoveSource(run_loop: Ref, source: Ref, mode: Ref);
    fn CFRunLoopRunInMode(mode: Ref, seconds: f64, return_after_source: bool) -> i32;
}

struct Context {
    root: u32,
    sender: tokio::sync::mpsc::Sender<()>,
}
unsafe extern "C" fn callback(context: Ref, _service: u32, message: u32, argument: Ref) {
    // The worker owns this allocation until its run-loop source is deregistered.
    let context = unsafe { &*(context as *const Context) };
    match message {
        DID_WAKE => {
            let _ = context.sender.try_send(());
        }
        CAN_SLEEP | WILL_SLEEP => unsafe {
            IOAllowPowerChange(context.root, argument as isize);
        },
        _ => {}
    }
}

pub(super) fn run(
    sender: tokio::sync::mpsc::Sender<()>,
    stop: Arc<AtomicBool>,
    ready: mpsc::SyncSender<()>,
) -> anyhow::Result<()> {
    let mut context = Box::new(Context { root: 0, sender });
    let mut port = ptr::null_mut();
    let mut notifier = 0;
    // IOKit delivers callbacks only while the worker processes this run loop.
    let root = unsafe {
        IORegisterForSystemPower(
            (&mut *context as *mut Context).cast(),
            &mut port,
            callback,
            &mut notifier,
        )
    };
    if root == 0 {
        anyhow::bail!("IOKit system power registration failed");
    }
    context.root = root;
    let source = unsafe { IONotificationPortGetRunLoopSource(port) };
    if source.is_null() {
        unsafe {
            IODeregisterForSystemPower(&mut notifier);
            IONotificationPortDestroy(port);
            IOServiceClose(root);
        }
        anyhow::bail!("IOKit system power run loop source is unavailable");
    }
    let run_loop = unsafe { CFRunLoopGetCurrent() };
    unsafe {
        CFRunLoopAddSource(run_loop, source, kCFRunLoopDefaultMode);
    }
    let _ = ready.send(());
    while !stop.load(Ordering::Acquire) {
        // This timeout bounds cancellation, not network probing or recovery.
        unsafe {
            CFRunLoopRunInMode(kCFRunLoopDefaultMode, 1.0, true);
        }
    }
    unsafe {
        CFRunLoopRemoveSource(run_loop, source, kCFRunLoopDefaultMode);
        IODeregisterForSystemPower(&mut notifier);
        IONotificationPortDestroy(port);
        IOServiceClose(root);
    }
    Ok(())
}
