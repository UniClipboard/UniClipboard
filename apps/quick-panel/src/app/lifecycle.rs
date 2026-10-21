//! Process lifecycle rules for running as a helper of the desktop GUI.

use std::io::{BufRead, BufReader, Read};

/// Command line flag telling the helper to exit once its standard input is closed.
///
/// The supervisor keeps the write end open while it runs, so an unclean GUI exit (crash, kill)
/// still ends the helper: the operating system closes the pipe. The Go host passes it as
/// `ExitWhenStdinCloses` in `packages/desktop-host-go/quickpanelhelper`.
pub const EXIT_WHEN_STDIN_CLOSES: &str = "--exit-when-stdin-closes";

/// A command from the supervising GUI: one JSON object per line on standard input (ADR-013 section
/// 10). The Go host writes them in `packages/desktop-host-go/quickpanelhelper`.
#[derive(Debug, PartialEq, Eq)]
pub enum HostCommand {
    /// Hold exactly these global shortcuts; the answer is written with the same `seq`.
    SetShortcuts { seq: u64, shortcuts: Vec<String> },
    /// Read the settings again (double tap, theme, language).
    ReloadSettings,
}

/// Decodes one line; anything that is not a known command is ignored, so the two sides can be
/// updated one at a time.
pub fn parse_command(line: &str) -> Option<HostCommand> {
    let value: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    match value.get("command")?.as_str()? {
        "set_shortcuts" => Some(HostCommand::SetShortcuts {
            seq: value.get("seq")?.as_u64()?,
            shortcuts: value
                .get("shortcuts")?
                .as_array()?
                .iter()
                .map(|item| item.as_str().map(str::to_owned))
                .collect::<Option<Vec<_>>>()?,
        }),
        "reload_settings" => Some(HostCommand::ReloadSettings),
        _ => None,
    }
}

/// Calls `on_closed` once `input` reaches end of file or fails, on a background thread, and hands
/// every command line of the GUI to `on_command` on the way.
///
/// The supervising GUI keeps the write end of this process's standard input open for as long as
/// it runs. When the GUI exits, crashes or is killed, the operating system closes that pipe, so
/// this is the one signal that also covers an unclean GUI exit, which no exit hook can.
pub fn watch_host(
    input: impl Read + Send + 'static,
    on_command: impl Fn(HostCommand) + Send + 'static,
    on_closed: impl FnOnce() + Send + 'static,
) {
    let spawned = std::thread::Builder::new()
        .name("parent-watch".into())
        .spawn(move || {
            let mut input = BufReader::new(input);
            let mut line = Vec::new();
            loop {
                line.clear();
                // `read_until` retries an interrupted read itself.
                match input.read_until(b'\n', &mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        if let Some(command) = parse_command(&String::from_utf8_lossy(&line)) {
                            on_command(command);
                        }
                    }
                }
            }
            on_closed();
        });
    if spawned.is_err() {
        tracing::warn!("Could not start the parent watcher");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::io::ErrorKind;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn closing_the_input_reports_that_the_parent_is_gone() {
        let (sender, receiver) = mpsc::channel();
        watch_host(
            Cursor::new(b"ignored bytes".to_vec()),
            |_| {},
            move || {
                sender.send(()).unwrap();
            },
        );
        receiver.recv_timeout(Duration::from_secs(5)).unwrap();
    }

    struct Broken;
    impl Read for Broken {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::new(ErrorKind::BrokenPipe, "closed"))
        }
    }

    #[test]
    fn a_failed_input_also_reports_that_the_parent_is_gone() {
        let (sender, receiver) = mpsc::channel();
        watch_host(Broken, |_| {}, move || sender.send(()).unwrap());
        receiver.recv_timeout(Duration::from_secs(5)).unwrap();
    }

    #[test]
    fn an_open_input_does_not_report_anything() {
        // A reader that never returns must not trigger the callback.
        struct Open(mpsc::Receiver<()>);
        impl Read for Open {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                let _ = self.0.recv();
                Ok(0)
            }
        }
        let (keep_open, blocked) = mpsc::channel::<()>();
        let (sender, receiver) = mpsc::channel();
        watch_host(Open(blocked), |_| {}, move || sender.send(()).unwrap());
        assert!(receiver.recv_timeout(Duration::from_millis(200)).is_err());
        drop(keep_open);
        receiver.recv_timeout(Duration::from_secs(5)).unwrap();
    }
}
