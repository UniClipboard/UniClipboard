use super::*;
use std::ffi::c_void;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::Power::{
    RegisterSuspendResumeNotification, UnregisterSuspendResumeNotification,
    DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DEVICE_NOTIFY_CALLBACK, PBT_APMRESUMEAUTOMATIC, PBT_APMRESUMESUSPEND,
};

unsafe extern "system" fn callback(
    context: *const c_void,
    event: u32,
    _setting: *const c_void,
) -> u32 {
    if event == PBT_APMRESUMEAUTOMATIC || event == PBT_APMRESUMESUSPEND {
        // Registration remains alive until the worker unregisters before dropping context.
        let sender = unsafe { &*(context as *const tokio::sync::mpsc::Sender<()>) };
        let _ = sender.try_send(());
    }
    0
}

pub(super) fn run(
    sender: tokio::sync::mpsc::Sender<()>,
    stop: Arc<AtomicBool>,
    ready: mpsc::SyncSender<()>,
) -> anyhow::Result<()> {
    let context = Box::new(sender);
    let mut parameters = DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS {
        Callback: Some(callback),
        Context: (&*context as *const tokio::sync::mpsc::Sender<()>)
            .cast::<c_void>()
            .cast_mut(),
    };
    let registration = unsafe {
        RegisterSuspendResumeNotification(
            HANDLE((&mut parameters as *mut DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS).cast()),
            DEVICE_NOTIFY_CALLBACK,
        )?
    };
    let _ = ready.send(());
    while !stop.load(Ordering::Acquire) {
        std::thread::park_timeout(Duration::from_secs(60));
    }
    let result = unsafe { UnregisterSuspendResumeNotification(registration) };
    if result.is_err() {
        // A failed deregistration may still call this context; retain it for memory safety.
        std::mem::forget(context);
    }
    result.map_err(anyhow::Error::new)
}
