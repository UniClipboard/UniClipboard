//! Offline collection of retained desktop role logs for startup recovery.

use std::fs::{self, File};
use std::io::{self, Read};
use std::path::Path;

use anyhow::{bail, Context, Result};
use zip::write::SimpleFileOptions;

/// Package existing role logs without connecting to the daemon or opening user storage.
pub fn export_startup_logs(logs_dir: &Path, destination: &Path) -> Result<()> {
    let mut files = Vec::new();
    for entry in fs::read_dir(logs_dir).context("read log directory")? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let is_role_log = ["gui", "daemon", "cli"].iter().any(|role| {
            name.strip_prefix(&format!("uniclipboard-{role}.json."))
                .is_some_and(|date| chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").is_ok())
        });
        if is_role_log {
            files.push((name.to_owned(), entry.path()));
        }
    }
    if files.is_empty() {
        bail!("No application logs are available to export");
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    let parent = destination
        .parent()
        .context("missing destination directory")?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    {
        let mut archive = zip::ZipWriter::new(temporary.as_file_mut());
        for (name, path) in files {
            let input = File::open(path).context("open application log")?;
            // Bound each snapshot so an actively growing log cannot keep export running.
            let length = input.metadata()?.len();
            archive.start_file(name, SimpleFileOptions::default().unix_permissions(0o600))?;
            io::copy(&mut input.take(length), &mut archive)?;
        }
        archive.finish()?;
    }
    temporary.as_file().sync_all()?;
    temporary.persist(destination).context("save log archive")?;
    Ok(())
}
