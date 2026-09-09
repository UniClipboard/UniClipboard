use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::time::{SystemTime, UNIX_EPOCH};

fn main() -> io::Result<()> {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("uc-flush-probe-{}-{nonce}", std::process::id()));
    let payload = b"synthetic file durability probe";
    let mut created = false;
    let result = (|| {
        let mut writer = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        created = true;
        writer.write_all(payload)?;
        writer.sync_all()?;
        drop(writer);

        let readonly = File::open(&path)?.sync_all();
        println!("platform={}", std::env::consts::OS);
        println!("readonly_sync_ok={}", readonly.is_ok());
        if let Err(error) = &readonly {
            println!(
                "readonly_error_kind={:?} os_code={:?}",
                error.kind(),
                error.raw_os_error()
            );
        }
        OpenOptions::new().write(true).open(&path)?.sync_all()?;
        if fs::read(&path)? != payload {
            return Err(io::Error::other("file contents changed"));
        }
        println!("writable_sync_ok=true contents_unchanged=true");
        #[cfg(windows)]
        if !matches!(readonly, Err(ref error) if error.kind() == io::ErrorKind::PermissionDenied) {
            return Err(io::Error::other("unexpected Windows readonly flush result"));
        }
        Ok(())
    })();
    let cleanup = if created {
        fs::remove_file(&path)
    } else {
        Ok(())
    };
    result.and(cleanup)
}
