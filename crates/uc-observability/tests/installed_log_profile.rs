//! The profile a process installed is a fact of that process, separate from
//! the persisted Debug Mode setting that only applies at the next start.

use uc_observability::{installed_log_profile, set_installed_log_profile, LogProfile};

#[test]
fn the_installed_profile_is_recorded_once() {
    assert_eq!(installed_log_profile(), None, "nothing installed yet");

    assert!(set_installed_log_profile(&LogProfile::Debug));
    assert_eq!(installed_log_profile(), Some("debug"));

    assert!(
        !set_installed_log_profile(&LogProfile::Prod),
        "the first installation stays authoritative"
    );
    assert_eq!(installed_log_profile(), Some("debug"));
}
