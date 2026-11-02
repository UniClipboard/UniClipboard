//! Exercise the offline archive boundary with task-owned synthetic log files.

use std::error::Error;
use std::path::PathBuf;
use uc_observability::startup_logs::export_startup_logs;

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args_os().skip(1);
    let logs = PathBuf::from(args.next().ok_or("missing synthetic log directory")?);
    let destination = PathBuf::from(args.next().ok_or("missing archive destination")?);
    export_startup_logs(&logs, &destination)?;
    Ok(())
}
