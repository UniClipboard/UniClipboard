//! Process lifecycle rules for running as a helper of the desktop GUI.

use std::io::{ErrorKind, Read};

/// Calls `on_closed` once `input` reaches end of file or fails, on a background thread.
///
/// The supervising GUI keeps the write end of this process's standard input open for as long as
/// it runs. When the GUI exits, crashes or is killed, the operating system closes that pipe, so
/// this is the one signal that also covers an unclean GUI exit, which no exit hook can.
pub fn watch_parent(
    mut input: impl Read + Send + 'static,
    on_closed: impl FnOnce() + Send + 'static,
) {
    let spawned = std::thread::Builder::new()
        .name("parent-watch".into())
        .spawn(move || {
            let mut buffer = [0_u8; 64];
            loop {
                match input.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(_) => {}
                    Err(error) if error.kind() == ErrorKind::Interrupted => {}
                    Err(_) => break,
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
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn closing_the_input_reports_that_the_parent_is_gone() {
        let (sender, receiver) = mpsc::channel();
        watch_parent(Cursor::new(b"ignored bytes".to_vec()), move || {
            sender.send(()).unwrap();
        });
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
        watch_parent(Broken, move || sender.send(()).unwrap());
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
        watch_parent(Open(blocked), move || sender.send(()).unwrap());
        assert!(receiver.recv_timeout(Duration::from_millis(200)).is_err());
        drop(keep_open);
        receiver.recv_timeout(Duration::from_secs(5)).unwrap();
    }
}
