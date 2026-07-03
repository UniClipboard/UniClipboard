//! Persisted cooldown for the Sparkle-style update prompt window.
//!
//! `last_notified` deduplicates per (channel, version), so a fast release
//! cadence still opens the updater window once for every new version — users
//! on a busy channel can be prompted several times a day. This store
//! remembers when the window was last opened and suppresses further
//! scheduler-triggered prompts until a per-channel cooldown has elapsed.
//! Versions found during the cooldown stay unrecorded in `last_notified`,
//! so the first prompt after the cooldown shows the newest version.
//!
//! Manual checks (tray menu) bypass the cooldown — the user explicitly asked
//! — but still refresh the timestamp when they open the window.
//!
//! Implementation mirrors `last_notified.rs`: single JSON file, self-healing
//! load, write-through persist.

use std::{
    io,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use tokio::fs;
use uc_core::settings::model::UpdateChannel;

/// Minimum wall-clock gap between two scheduler-triggered prompt windows on
/// the stable channel.
pub const STABLE_PROMPT_COOLDOWN_SECS: i64 = 72 * 60 * 60;
/// Cooldown for pre-release channels (alpha/beta/rc). Shorter than stable:
/// these users opted into frequent releases, but even they don't need a
/// window per same-day release.
pub const PRERELEASE_PROMPT_COOLDOWN_SECS: i64 = 24 * 60 * 60;

/// Cooldown applied to scheduler-triggered prompts for the given channel.
pub fn cooldown_secs(channel: &UpdateChannel) -> i64 {
    match channel {
        UpdateChannel::Stable => STABLE_PROMPT_COOLDOWN_SECS,
        UpdateChannel::Alpha | UpdateChannel::Beta | UpdateChannel::Rc => {
            PRERELEASE_PROMPT_COOLDOWN_SECS
        }
    }
}

/// Epoch-seconds record of the last time the updater window was opened.
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PromptThrottleStore {
    /// `None` = never prompted (or the file was missing/corrupted).
    last_prompt_at: Option<i64>,
}

impl PromptThrottleStore {
    /// Read from disk; missing, empty or unparsable files all yield an empty
    /// store (self-healing semantics, same as the sibling stores).
    pub async fn load(path: &Path) -> Self {
        match fs::read(path).await {
            Ok(bytes) if bytes.is_empty() => Self::default(),
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|err| {
                tracing::warn!(
                    target: "update_scheduler::prompt_throttle",
                    error = %err,
                    path = %path.display(),
                    "corrupted update_prompt_throttle.json; treating as empty"
                );
                Self::default()
            }),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Self::default(),
            Err(err) => {
                tracing::warn!(
                    target: "update_scheduler::prompt_throttle",
                    error = %err,
                    path = %path.display(),
                    "failed to read update_prompt_throttle.json; treating as empty"
                );
                Self::default()
            }
        }
    }

    /// Whether a scheduler-triggered prompt is currently allowed for
    /// `channel`.
    pub fn allows_prompt(&self, channel: &UpdateChannel) -> bool {
        self.allows_prompt_at(channel, current_epoch_secs())
    }

    fn allows_prompt_at(&self, channel: &UpdateChannel, now_secs: i64) -> bool {
        let Some(last) = self.last_prompt_at else {
            return true;
        };
        let elapsed = now_secs - last;
        // Negative elapsed means the stored timestamp is in the future (clock
        // rollback or a corrupted value). Allow the prompt instead of
        // suppressing until the bogus timestamp is eventually passed; the
        // next successful open overwrites it with a sane value.
        elapsed < 0 || elapsed >= cooldown_secs(channel)
    }

    /// Record "prompted now" and persist immediately.
    pub async fn record_prompt_now(&mut self, path: &Path) -> io::Result<()> {
        self.last_prompt_at = Some(current_epoch_secs());
        self.persist(path).await
    }

    async fn persist(&self, path: &Path) -> io::Result<()> {
        let bytes = serde_json::to_vec_pretty(self).map_err(io::Error::other)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::write(path, bytes).await
    }
}

/// Current epoch seconds; clamps to 0 for pre-epoch clocks.
fn current_epoch_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn store_path(dir: &TempDir) -> std::path::PathBuf {
        dir.path().join("update_prompt_throttle.json")
    }

    #[tokio::test]
    async fn load_returns_empty_when_file_missing() {
        let dir = TempDir::new().unwrap();
        let store = PromptThrottleStore::load(&store_path(&dir)).await;
        assert_eq!(store, PromptThrottleStore::default());
        assert!(store.allows_prompt(&UpdateChannel::Stable));
    }

    #[tokio::test]
    async fn record_persists_and_round_trips() {
        let dir = TempDir::new().unwrap();
        let path = store_path(&dir);

        let mut store = PromptThrottleStore::load(&path).await;
        store.record_prompt_now(&path).await.unwrap();

        let reloaded = PromptThrottleStore::load(&path).await;
        assert!(!reloaded.allows_prompt(&UpdateChannel::Stable));
        assert!(!reloaded.allows_prompt(&UpdateChannel::Alpha));
    }

    #[test]
    fn allows_when_never_prompted() {
        let store = PromptThrottleStore::default();
        assert!(store.allows_prompt_at(&UpdateChannel::Stable, 1_000_000));
    }

    #[test]
    fn blocks_within_cooldown_and_allows_after() {
        let last = 1_000_000_i64;
        let store = PromptThrottleStore {
            last_prompt_at: Some(last),
        };

        for (channel, cooldown) in [
            (UpdateChannel::Stable, STABLE_PROMPT_COOLDOWN_SECS),
            (UpdateChannel::Alpha, PRERELEASE_PROMPT_COOLDOWN_SECS),
            (UpdateChannel::Beta, PRERELEASE_PROMPT_COOLDOWN_SECS),
            (UpdateChannel::Rc, PRERELEASE_PROMPT_COOLDOWN_SECS),
        ] {
            assert!(
                !store.allows_prompt_at(&channel, last + cooldown - 1),
                "{channel:?} should be blocked just before cooldown expiry"
            );
            assert!(
                store.allows_prompt_at(&channel, last + cooldown),
                "{channel:?} should be allowed exactly at cooldown expiry"
            );
        }
    }

    #[test]
    fn future_timestamp_does_not_suppress_forever() {
        // Clock rollback: stored timestamp is ahead of "now". The prompt must
        // stay available rather than being locked out until the bogus
        // timestamp is passed.
        let store = PromptThrottleStore {
            last_prompt_at: Some(2_000_000),
        };
        assert!(store.allows_prompt_at(&UpdateChannel::Stable, 1_000_000));
    }

    #[test]
    fn cooldown_constants_match_plan() {
        // Lock in the agreed cadence: 72h stable, 24h pre-release.
        assert_eq!(STABLE_PROMPT_COOLDOWN_SECS, 72 * 60 * 60);
        assert_eq!(PRERELEASE_PROMPT_COOLDOWN_SECS, 24 * 60 * 60);
    }
}
