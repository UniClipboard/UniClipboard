pub mod app_session;
#[cfg(feature = "dev-tools")]
pub mod blob;
#[cfg(feature = "dev-tools")]
pub mod dev;
pub mod devices;
#[cfg(feature = "dev-tools")]
pub mod dump_clipboard;
pub mod get;
pub mod init;
pub mod invite;
pub mod join;
pub mod members;
pub mod mobile_sync;
#[cfg(feature = "dev-tools")]
pub mod probe;
pub mod recv;
pub mod search;
#[cfg(feature = "dev-tools")]
pub mod seed_clipboard;
pub mod send;
pub mod start;
pub mod status;
pub mod stop;
pub mod switch_space;
pub mod upgrade;
pub mod watch;

/// Render a daemon-call failure for terminal output: prefer the daemon's
/// human-readable error message (`DaemonRequestError::Status.message`) over
/// the full "daemon request <path> failed with status ..." string, which is
/// noise in a user-facing error line.
pub(crate) fn daemon_error_message(err: &anyhow::Error) -> String {
    err.downcast_ref::<uc_daemon_client::DaemonRequestError>()
        .and_then(uc_daemon_client::DaemonRequestError::message)
        .map(str::to_string)
        .unwrap_or_else(|| err.to_string())
}
