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

use serde_json::{Map, Value};
use tokio::sync::{broadcast, Mutex};
use tracing::warn;

/// File name of the product settings, next to the Engine's `settings.json`
/// in the profile's app data root.
pub const FILE_NAME: &str = "desktop-settings.json";

/// Current file format version, written when the file has none.
const FORMAT_VERSION: u64 = 1;

/// Largest file read back; the settings are a few enums, far below this.
const MAX_BYTES: u64 = 1024 * 1024;

/// Buffered notifications per subscriber before it lags.
const CHANGE_CAPACITY: usize = 64;

const TRAY_ICON_PATH: &str = "general.trayIcon";

/// The tray icon artwork.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TrayIcon {
    /// The monochrome app glyph.
    #[default]
    Glyph,
    /// The cat artwork.
    Cat,
}

impl TrayIcon {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Glyph => "glyph",
            Self::Cat => "cat",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "glyph" => Some(Self::Glyph),
            "cat" => Some(Self::Cat),
            _ => None,
        }
    }
}

/// Effective product settings: every field has a valid value.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DesktopSettings {
    pub general: GeneralSettings,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GeneralSettings {
    pub tray_icon: TrayIcon,
}

/// An explicit write. `None` leaves the field as it is on disk.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DesktopSettingsPatch {
    pub general: GeneralSettingsPatch,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GeneralSettingsPatch {
    pub tray_icon: Option<TrayIcon>,
}

/// A top-level settings section named in a change notification.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SettingsSection {
    General,
}

impl SettingsSection {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::General => "general",
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
    Write(io::Error),
}

impl std::fmt::Display for DesktopSettingsError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Write(error) => write!(formatter, "failed to write product settings: {error}"),
        }
    }
}

impl std::error::Error for DesktopSettingsError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
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
            path,
            state: Mutex::new(State {
                raw,
                settings,
                revision: 0,
            }),
            changes,
        }
    }

    pub async fn snapshot(&self) -> DesktopSettingsSnapshot {
        let state = self.state.lock().await;
        DesktopSettingsSnapshot {
            settings: state.settings.clone(),
            revision: state.revision,
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<DesktopSettingsChanged> {
        self.changes.subscribe()
    }

    /// Write the fields `patch` names. The file is replaced atomically, and
    /// only after that succeeds do the memory state and revision advance and
    /// the notification go out. A write that leaves the document unchanged
    /// does none of that.
    pub async fn apply(
        &self,
        patch: DesktopSettingsPatch,
    ) -> Result<ApplyResult, DesktopSettingsError> {
        let mut state = self.state.lock().await;
        let mut sections = Vec::new();
        let mut next = state.raw.clone();
        if let Some(icon) = patch.general.tray_icon {
            let document = next.get_or_insert_with(Map::new);
            document
                .entry("version")
                .or_insert_with(|| Value::from(FORMAT_VERSION));
            let mut general = take_object(document, "general");
            general.insert(
                "trayIcon".to_owned(),
                Value::String(icon.as_str().to_owned()),
            );
            document.insert("general".to_owned(), Value::Object(general));
            record_user_write(document, TRAY_ICON_PATH);
            sections.push(SettingsSection::General);
        }

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

/// Derive valid values from the raw document. Warnings name the field path,
/// never the stored value.
fn effective(raw: Option<&Map<String, Value>>, warn_invalid: bool) -> DesktopSettings {
    let mut settings = DesktopSettings::default();
    let stored = raw
        .and_then(|document| document.get("general"))
        .and_then(|general| general.get("trayIcon"));
    if let Some(value) = stored {
        match value.as_str().and_then(TrayIcon::parse) {
            Some(icon) => settings.general.tray_icon = icon,
            None if warn_invalid => warn!(
                error_kind = "desktop_settings_invalid_field",
                field = TRAY_ICON_PATH,
                "Invalid product setting; using its default"
            ),
            None => {}
        }
    }
    settings
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
    match serde_json::from_slice::<Value>(&bytes) {
        Ok(Value::Object(document)) => Ok(Some(document)),
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
