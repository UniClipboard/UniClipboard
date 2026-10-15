//! The daemon claims its profile (the instance lock) before it installs any
//! file logging, and a failed claim leaves a bounded startup failure record.

use std::fs;
use std::time::Duration;

use uc_daemon::daemon::profile_claim::claim_instance_lock;
use uc_daemon_local::instance_lock::DaemonInstanceLock;
use uc_observability::startup_failure::{
    read_startup_failure, StartupFailureClass, StartupStep, STARTUP_FAILURE_FILE,
};

#[tokio::test]
async fn a_busy_profile_fails_with_a_record_and_opens_no_log_file() {
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("data");
    let logs = root.path().join("logs");
    let _holder = DaemonInstanceLock::try_acquire(&data).unwrap();

    let error = claim_instance_lock(&data, &logs, Duration::from_millis(100))
        .await
        .expect_err("the profile is held by another daemon");

    assert!(!error.to_string().is_empty());
    let record = read_startup_failure(&logs).unwrap().expect("record");
    assert_eq!(record.step, StartupStep::InstanceLock);
    assert_eq!(record.class, StartupFailureClass::LockTimeout);
    assert_eq!(record.pid, std::process::id());
    let names: Vec<_> = fs::read_dir(&logs)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(
        names,
        [STARTUP_FAILURE_FILE],
        "a daemon that lost the profile must not open Engine or host log files"
    );
}

#[tokio::test]
async fn a_free_profile_is_claimed_without_a_record() {
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("data");
    let logs = root.path().join("logs");

    let lock = claim_instance_lock(&data, &logs, Duration::from_millis(100))
        .await
        .expect("free profile");

    assert!(read_startup_failure(&logs).unwrap().is_none());
    assert!(
        DaemonInstanceLock::try_acquire(&data).is_err(),
        "the claim holds the lock"
    );
    drop(lock);
    DaemonInstanceLock::try_acquire(&data).expect("released on drop");
}

#[tokio::test]
async fn an_unusable_data_directory_is_recorded_as_an_io_failure() {
    let root = tempfile::tempdir().unwrap();
    let blocker = root.path().join("not-a-directory");
    fs::write(&blocker, "file").unwrap();
    let logs = root.path().join("logs");

    claim_instance_lock(&blocker.join("data"), &logs, Duration::from_millis(100))
        .await
        .expect_err("the data directory cannot be created");

    let record = read_startup_failure(&logs).unwrap().expect("record");
    assert_eq!(record.step, StartupStep::InstanceLock);
    assert_eq!(record.class, StartupFailureClass::Io);
}
