//! Opt-in control for the native end-to-end tests, so that they can run beside the user's own
//! session without taking over the keyboard.
//!
//! With `UC_GPUI_TEST_CONTROL=stdin` the panel registers no global shortcut and no modifier
//! double tap; instead each `toggle` line on standard input opens or closes it, and the end of
//! standard input exits the process, as `--exit-when-stdin-closes` does. Without the variable
//! nothing here runs.

use std::io::BufRead;

use super::triggers::Trigger;

const ENV: &str = "UC_GPUI_TEST_CONTROL";

pub fn enabled() -> bool {
    std::env::var(ENV).as_deref() == Ok("stdin")
}

/// Turns `toggle` lines on standard input into triggers, on a background thread.
pub fn read_stdin(send: async_channel::Sender<Trigger>) {
    let spawned = std::thread::Builder::new()
        .name("test-control".into())
        .spawn(move || {
            for line in std::io::stdin().lock().lines() {
                match line {
                    Ok(line) if line.trim() == "toggle" => {
                        let _ = send.try_send(Trigger::TestControl);
                    }
                    Ok(_) => {}
                    Err(_) => break,
                }
            }
            std::process::exit(0);
        });
    if spawned.is_err() {
        tracing::error!("Could not start the test control reader");
        std::process::exit(1);
    }
}
