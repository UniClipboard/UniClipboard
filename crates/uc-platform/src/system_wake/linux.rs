use super::*;
use dbus::blocking::Connection;
use dbus::message::MatchRule;

pub(super) fn run(
    sender: tokio::sync::mpsc::Sender<()>,
    stop: Arc<AtomicBool>,
    ready: mpsc::SyncSender<()>,
) -> anyhow::Result<()> {
    let connection = Connection::new_system()?;
    let rule = MatchRule::new_signal("org.freedesktop.login1.Manager", "PrepareForSleep")
        .with_sender("org.freedesktop.login1")
        .with_path("/org/freedesktop/login1");
    let token = connection.add_match(rule, move |(sleeping,): (bool,), _, _| {
        if !sleeping {
            let _ = sender.try_send(());
        }
        true
    })?;
    let _ = ready.send(());
    while !stop.load(Ordering::Acquire) {
        connection.process(Duration::from_secs(1))?;
    }
    connection.remove_match(token)?;
    Ok(())
}
