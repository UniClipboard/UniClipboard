//! Desktop product settings: preferences of the Desktop shell (tray, windows,
//! OS integration) that the portable Engine neither reads nor owns.
//!
//! The daemon constructs one [`DesktopSettingsStore`] per profile, after it
//! holds the profile instance lock and before the Engine starts, and injects
//! it into its API state. The file is plain JSON: it is configuration under
//! `VISION.md` and must never hold clipboard content, data derived from it,
//! or credentials. Rules: `docs/adr/adr-013-desktop-product-settings.md`.
//!
//! The store keeps the raw document as read from disk. Effective values are
//! derived field by field, so a missing or invalid field falls back to its
//! default without failing the load, and a write replaces only the paths it
//! names: unknown keys and invalid values written by another version stay on
//! disk until the user sets that field.

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Map, Value};
use tokio::sync::{broadcast, Mutex};
use tracing::warn;

mod fields;
mod grammar;
mod legacy;
mod model;

pub use fields::InvalidField;
pub use legacy::{import_legacy, ImportOutcome, CURRENT_FIELD_SET};
pub use model::{
    DesktopSettings, DesktopSettingsPatch, DoubleTapModifier, GeneralSettings,
    GeneralSettingsPatch, QuickPanelPosition, QuickPanelSettings, QuickPanelSettingsPatch,
    ShortcutBinding, StartupMode, Theme, TrayIcon, UpdateChannel,
};

/// File name of the product settings, next to the Engine's `settings.json`
/// in the profile's app data root.
pub const FILE_NAME: &str = "desktop-settings.json";

/// Current file format version, written when the file has none.
const FORMAT_VERSION: u64 = 1;

/// Largest file read back; the settings are a few enums, far below this.
const MAX_BYTES: u64 = 1024 * 1024;

/// Buffered notifications per subscriber before it lags.
const CHANGE_CAPACITY: usize = 64;

/// A top-level settings section named in a change notification.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SettingsSection {
    General,
    KeyboardShortcuts,
    QuickPanel,
}

impl SettingsSection {
    /// Every section, for a subscriber that must report all of them after missing notifications.
    pub const ALL: &'static [SettingsSection] =
        &[Self::General, Self::KeyboardShortcuts, Self::QuickPanel];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::General => "general",
            Self::KeyboardShortcuts => "keyboardShortcuts",
            Self::QuickPanel => "quickPanel",
        }
    }
}

/// Sent after a write changed the file. It carries no values: subscribers
/// read a fresh [`DesktopSettingsSnapshot`], and re-read after a lag.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DesktopSettingsChanged {
    pub revision: u64,
    pub sections: Vec<SettingsSection>,
}

/// Effective settings with the revision they belong to. The revision lives
/// in memory only: it starts at 0 with each daemon run and grows by one per
/// persisted change.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DesktopSettingsSnapshot {
    pub settings: DesktopSettings,
    pub revision: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApplyResult {
    pub snapshot: DesktopSettingsSnapshot,
    /// False when the write left the file as it was (same value, empty patch).
    pub changed: bool,
}

#[derive(Debug)]
pub enum DesktopSettingsError {
    /// A value failed its grammar; nothing was written.
    Invalid(InvalidField),
    Write(io::Error),
}

impl std::fmt::Display for DesktopSettingsError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(error) => error.fmt(formatter),
            Self::Write(error) => write!(formatter, "failed to write product settings: {error}"),
        }
    }
}

impl std::error::Error for DesktopSettingsError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Invalid(error) => Some(error),
            Self::Write(error) => Some(error),
        }
    }
}

struct State {
    /// The document as on disk; `None` when the file is missing or unusable.
    raw: Option<Map<String, Value>>,
    settings: DesktopSettings,
    revision: u64,
}

/// The single writer of the product settings file.
pub struct DesktopSettingsStore {
    inner: Arc<Inner>,
}

struct Inner {
    path: Option<PathBuf>,
    state: Mutex<State>,
    changes: broadcast::Sender<DesktopSettingsChanged>,
}

impl DesktopSettingsStore {
    /// Load the settings at `path`. Never fails and never writes: a missing,
    /// unreadable, oversized or malformed file yields the defaults and stays
    /// untouched until the next explicit write.
    pub fn load(path: PathBuf) -> Self {
        let raw = match read(&path) {
            Ok(raw) => raw,
            Err(reason) => {
                warn!(
                    error_kind = "desktop_settings_read",
                    reason, "Product settings unusable; using defaults until the next write"
                );
                None
            }
        };
        let settings = effective(raw.as_ref(), true);
        Self::with_state(Some(path), raw, settings)
    }

    /// A store without a file, for assembly paths that do not wire one.
    pub fn in_memory() -> Self {
        Self::with_state(None, None, DesktopSettings::default())
    }

    fn with_state(
        path: Option<PathBuf>,
        raw: Option<Map<String, Value>>,
        settings: DesktopSettings,
    ) -> Self {
        let (changes, _) = broadcast::channel(CHANGE_CAPACITY);
        Self {
            inner: Arc::new(Inner {
                path,
                state: Mutex::new(State {
                    raw,
                    settings,
                    revision: 0,
                }),
                changes,
            }),
        }
    }

    pub async fn snapshot(&self) -> DesktopSettingsSnapshot {
        let state = self.inner.state.lock().await;
        DesktopSettingsSnapshot {
            settings: state.settings.clone(),
            revision: state.revision,
        }
    }

    /// Check `patch` against the grammars and the current document without
    /// writing, so a caller can reject it before writing other stores.
    pub async fn validate(&self, patch: &DesktopSettingsPatch) -> Result<(), InvalidField> {
        let state = self.inner.state.lock().await;
        merge(&state, patch).map(|_| ())
    }

    pub fn subscribe(&self) -> broadcast::Receiver<DesktopSettingsChanged> {
        self.inner.changes.subscribe()
    }

    /// Write the fields `patch` names. The file is replaced atomically, and
    /// only after that succeeds do the memory state and revision advance and
    /// the notification go out. A write that leaves the document unchanged
    /// does none of that.
    ///
    /// The write runs on its own task, so a caller that is dropped mid-write
    /// (a cancelled HTTP request) cannot leave the file changed while memory,
    /// revision and notification stay behind.
    pub async fn apply(
        &self,
        patch: DesktopSettingsPatch,
    ) -> Result<ApplyResult, DesktopSettingsError> {
        let inner = Arc::clone(&self.inner);
        tokio::spawn(async move { inner.apply(patch).await })
            .await
            .map_err(|error| DesktopSettingsError::Write(io::Error::other(error)))?
    }
}

impl Inner {
    async fn apply(
        &self,
        patch: DesktopSettingsPatch,
    ) -> Result<ApplyResult, DesktopSettingsError> {
        let mut state = self.state.lock().await;
        let (next, sections) = merge(&state, &patch).map_err(DesktopSettingsError::Invalid)?;

        if next == state.raw {
            return Ok(ApplyResult {
                snapshot: DesktopSettingsSnapshot {
                    settings: state.settings.clone(),
                    revision: state.revision,
                },
                changed: false,
            });
        }

        if let (Some(path), Some(document)) = (&self.path, &next) {
            let bytes = serde_json::to_vec_pretty(document)
                .map_err(|error| DesktopSettingsError::Write(error.into()))?;
            let path = path.clone();
            tokio::task::spawn_blocking(move || write_atomic(&path, &bytes))
                .await
                .map_err(|error| DesktopSettingsError::Write(io::Error::other(error)))?
                .map_err(DesktopSettingsError::Write)?;
        }

        state.settings = effective(next.as_ref(), false);
        state.raw = next;
        state.revision += 1;
        // Sent under the lock so notifications arrive in revision order. No
        // receiver is not an error: nobody has subscribed yet.
        let _ = self.changes.send(DesktopSettingsChanged {
            revision: state.revision,
            sections,
        });
        Ok(ApplyResult {
            snapshot: DesktopSettingsSnapshot {
                settings: state.settings.clone(),
                revision: state.revision,
            },
            changed: true,
        })
    }
}

/// The document after a patch, and the sections the patch touches.
type Merged = (Option<Map<String, Value>>, Vec<SettingsSection>);

/// The document after `patch`, and the sections it touches. Fails, without
/// side effects, when a value does not pass its grammar.
fn merge(state: &State, patch: &DesktopSettingsPatch) -> Result<Merged, InvalidField> {
    if patch
        .general
        .theme_color
        .as_deref()
        .is_some_and(|name| !grammar::is_preset_name(name))
    {
        return Err(InvalidField {
            path: "general.themeColor".to_owned(),
        });
    }
    let entries = fields::patch_entries(patch, &state.settings);
    fields::validate_entries(&entries)?;
    let mut sections = Vec::new();
    let mut next = state.raw.clone();
    if entries.is_empty() {
        return Ok((next, sections));
    }
    let document = next.get_or_insert_with(Map::new);
    document
        .entry("version")
        .or_insert_with(|| Value::from(FORMAT_VERSION));
    for entry in &entries {
        fields::set_member(document, entry.section, &entry.key, entry.value.clone());
        record_user_write(document, &entry.path);
        if let Some(section) = fields::section_of_path(&entry.path) {
            if !sections.contains(&section) {
                sections.push(section);
            }
        }
    }
    let shortcut_ids = document
        .get(fields::SHORTCUTS)
        .and_then(Value::as_object)
        .map_or(0, Map::len);
    if !grammar::shortcut_id_count_ok(shortcut_ids) {
        return Err(InvalidField {
            path: fields::SHORTCUTS.to_owned(),
        });
    }
    Ok((next, sections))
}

/// Derive valid values from the raw document. Warnings name the field path,
/// never the stored value.
fn effective(raw: Option<&Map<String, Value>>, warn_invalid: bool) -> DesktopSettings {
    let values = fields::normalized(raw, |path| {
        if warn_invalid {
            warn!(
                error_kind = "desktop_settings_invalid_field",
                field = path,
                "Invalid product setting; using its default"
            );
        }
    });
    fields::typed(&values)
}

/// Remove the object under `key` for editing; a value of any other type is
/// replaced by an empty object. The caller puts the edited object back.
fn take_object(document: &mut Map<String, Value>, key: &str) -> Map<String, Value> {
    match document.remove(key) {
        Some(Value::Object(map)) => map,
        _ => Map::new(),
    }
}

/// Add `path` to `import.userWritten`, the fields the legacy importer must
/// not overwrite. Other `import` keys are kept as they are.
fn record_user_write(document: &mut Map<String, Value>, path: &str) {
    let mut import = take_object(document, "import");
    let mut paths = match import.remove("userWritten") {
        Some(Value::Array(paths)) => paths,
        _ => Vec::new(),
    };
    if !paths.iter().any(|existing| existing.as_str() == Some(path)) {
        paths.push(Value::String(path.to_owned()));
    }
    import.insert("userWritten".to_owned(), Value::Array(paths));
    document.insert("import".to_owned(), Value::Object(import));
}

/// Read the document. `Ok(None)` for a missing file; `Err` carries a reason
/// category for the log, never file content.
fn read(path: &Path) -> Result<Option<Map<String, Value>>, &'static str> {
    match read_bytes(path)? {
        Some(bytes) => parse_document(&bytes).map(Some),
        None => Ok(None),
    }
}

/// Read at most [`MAX_BYTES`]. `Ok(None)` for a missing file.
fn read_bytes(path: &Path) -> Result<Option<Vec<u8>>, &'static str> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("unreadable"),
    };
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "unreadable")?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("too_large");
    }
    Ok(Some(bytes))
}

fn parse_document(bytes: &[u8]) -> Result<Map<String, Value>, &'static str> {
    match serde_json::from_slice::<Value>(bytes) {
        Ok(Value::Object(document)) => Ok(document),
        Ok(_) => Err("not_an_object"),
        Err(_) => Err("malformed_json"),
    }
}

/// Write `bytes` to `path` through a synced temporary file in the same
/// directory, so a crash leaves either the old file or the new one.
fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("missing product settings directory"))?;
    std::fs::create_dir_all(parent)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(bytes)?;
    temporary.flush()?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|error| error.error)?;
    Ok(())
}
