//! One-time import of product fields from the Engine `settings.json`
//! (ADR-013 section 8.2). Bootstrap runs it before any Engine settings load.
//!
//! The legacy file is read as raw JSON and never written. The marker is
//! `import.fieldSet`, advanced in the same atomic write as the imported values.
//! Logs carry field paths, reason categories and hashes, never values.

use std::collections::BTreeMap;
use std::io;
use std::path::Path;

use serde_json::{Map, Value};
use tracing::{info, warn};

use crate::fields::{
    self, member, set_member, shortcut_path, LEGACY_SHORTCUTS, SCALARS, SHORTCUTS,
};
use crate::{
    effective, grammar, parse_document, read, read_bytes, take_object, write_atomic,
    DesktopSettings, FORMAT_VERSION,
};

/// The highest field set this binary imports.
pub const CURRENT_FIELD_SET: u64 = 1;

#[derive(Debug)]
pub enum ImportOutcome {
    /// Values were copied and the file written. `repeated` is a D8 import after
    /// the legacy values changed; `paths` are the product paths that changed.
    Imported { repeated: bool, paths: Vec<String> },
    /// No legacy file: the marker advanced without values.
    MarkedWithoutLegacy,
    /// Marker current and legacy values unchanged: nothing written.
    Unchanged,
    /// Not run this start (reason category); the next start retries.
    Deferred(&'static str),
    /// The product file write failed; both files are unchanged.
    WriteFailed(io::Error),
}

/// Run the import and return its outcome with the resulting effective
/// settings (no field warnings; the store logs those when it loads).
pub fn import_legacy(product_path: &Path, legacy_path: &Path) -> (ImportOutcome, DesktopSettings) {
    // A product file that cannot be used is kept as it is until an explicit
    // write replaces it (ADR-013 section 3), so the import waits for that.
    let mut document = match read(product_path) {
        Ok(document) => document.unwrap_or_default(),
        Err(reason) => {
            warn!(
                error_kind = "desktop_settings_import_deferred",
                reason,
                "Product settings unusable; legacy import deferred until the file is usable"
            );
            return (ImportOutcome::Deferred(reason), effective(None, false));
        }
    };
    let outcome = import_into(&mut document, product_path, legacy_path);
    let settings = match &outcome {
        ImportOutcome::Imported { .. } | ImportOutcome::MarkedWithoutLegacy => {
            effective(Some(&document), false)
        }
        _ => effective(read(product_path).ok().flatten().as_ref(), false),
    };
    (outcome, settings)
}

fn import_into(
    document: &mut Map<String, Value>,
    product_path: &Path,
    legacy_path: &Path,
) -> ImportOutcome {
    let import = document.get("import").and_then(Value::as_object);
    let stored_set = import
        .and_then(|import| import.get("fieldSet"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let stored_fingerprint = import
        .and_then(|import| import.get("sourceFingerprint"))
        .and_then(Value::as_str)
        .map(str::to_owned);

    let bytes = match read_bytes(legacy_path) {
        Ok(Some(bytes)) => bytes,
        Ok(None) => {
            if stored_set >= CURRENT_FIELD_SET {
                info!(
                    field_set = stored_set,
                    "Legacy settings missing; nothing to import"
                );
                return ImportOutcome::Unchanged;
            }
            // The fingerprint of "no legacy values": when the Engine later
            // writes its defaults, the normalized set is the same and no
            // import follows.
            write_marker(document, Some(&fingerprint(&derive(&Map::new()))));
            return match persist(document, product_path) {
                Ok(()) => {
                    info!(
                        field_set = CURRENT_FIELD_SET,
                        "Legacy settings missing; import marker advanced without values"
                    );
                    ImportOutcome::MarkedWithoutLegacy
                }
                Err(error) => write_failed(error),
            };
        }
        Err(reason) => return defer(reason),
    };
    let source_hash = blake3::hash(&bytes).to_hex().to_string();
    let legacy = match parse_document(&bytes) {
        Ok(legacy) => legacy,
        Err(reason) => return defer(reason),
    };

    let derived = derive(&legacy);
    let fingerprint = fingerprint(&derived);
    let repeated = if stored_set < CURRENT_FIELD_SET {
        false
    } else if stored_set > CURRENT_FIELD_SET {
        // A newer binary owns the file; its Engine may have erased the legacy
        // fields, so their absence is not a user edit.
        info!(
            field_set = stored_set,
            legacy_settings_blake3 = %source_hash,
            "Product settings come from a newer field set; legacy import skipped"
        );
        return ImportOutcome::Unchanged;
    } else if stored_fingerprint.as_deref() == Some(fingerprint.as_str()) {
        info!(
            field_set = stored_set,
            legacy_settings_blake3 = %source_hash,
            "Legacy settings unchanged since the last import"
        );
        return ImportOutcome::Unchanged;
    } else {
        true
    };

    let user_written: Vec<String> = document
        .get("import")
        .and_then(|import| import.get("userWritten"))
        .and_then(Value::as_array)
        .map(|paths| {
            paths
                .iter()
                .filter_map(|path| path.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    let before = fields::normalized(Some(document), |_| {});
    let mut changed = Vec::new();
    let mut invalid = Vec::new();
    let mut kept = Vec::new();
    for (path, (section, key, value)) in &derived {
        if user_written.contains(path) {
            let legacy_normalized = normalize_one(path, value.as_ref());
            if before.get(path) != legacy_normalized.as_ref() {
                kept.push(path.clone());
            }
            continue;
        }
        if member(Some(document), section, key) != value.as_ref() {
            changed.push(path.clone());
        }
        if value
            .as_ref()
            .is_some_and(|value| normalize_one(path, Some(value)).as_ref() != Some(value))
        {
            invalid.push(path.clone());
        }
        set_member(document, section, key, value.clone());
    }
    // Shortcut ids that an earlier import copied and the legacy file no longer has.
    let stale: Vec<String> = document
        .get(SHORTCUTS)
        .and_then(Value::as_object)
        .map(|shortcuts| {
            shortcuts
                .keys()
                .filter(|id| {
                    let path = shortcut_path(id);
                    !derived.contains_key(&path) && !user_written.contains(&path)
                })
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    for id in stale {
        changed.push(shortcut_path(&id));
        set_member(document, SHORTCUTS, &id, None);
    }

    write_marker(document, Some(&fingerprint));
    if let Err(error) = persist(document, product_path) {
        return write_failed(error);
    }
    info!(
        field_set = CURRENT_FIELD_SET,
        repeated,
        legacy_settings_blake3 = %source_hash,
        imported = ?changed,
        "Imported product settings from the legacy Engine settings"
    );
    if !invalid.is_empty() {
        warn!(
            error_kind = "desktop_settings_import_invalid_field",
            fields = ?invalid,
            "Legacy values kept as stored but not effective; their defaults apply"
        );
    }
    if !kept.is_empty() {
        warn!(
            error_kind = "desktop_settings_import_kept_user_value",
            fields = ?kept,
            "Legacy values differ from values written under this version; the written values stay"
        );
    }
    ImportOutcome::Imported {
        repeated,
        paths: changed,
    }
}

type Derived = BTreeMap<String, (&'static str, String, Option<Value>)>;

/// The legacy value of every field-set-1 path, with the legacy rules applied
/// (ADR-013 section 8.2 step 4). `None` means absent.
fn derive(legacy: &Map<String, Value>) -> Derived {
    let mut derived = Derived::new();
    let legacy = Some(legacy);
    let non_null = |value: Option<&Value>| value.filter(|value| !value.is_null()).cloned();
    for scalar in SCALARS {
        let Some((legacy_section, legacy_key)) = scalar.legacy else {
            continue;
        };
        let value = match scalar.key {
            "startupMode" => member(legacy, legacy_section, legacy_key)
                .cloned()
                .or_else(|| {
                    let flag = |key| member(legacy, "general", key).and_then(Value::as_bool);
                    if flag("lightweight_start") == Some(true) {
                        Some(Value::String("lightweight".to_owned()))
                    } else if flag("silent_start") == Some(true) {
                        Some(Value::String("silent".to_owned()))
                    } else {
                        None
                    }
                }),
            "themeColorLight" | "themeColorDark" => {
                let split = member(legacy, legacy_section, legacy_key);
                non_null(split)
                    .or_else(|| non_null(member(legacy, "general", "theme_color")))
                    .or_else(|| split.cloned())
            }
            _ => member(legacy, legacy_section, legacy_key).cloned(),
        };
        derived.insert(
            scalar.path(),
            (scalar.section, scalar.key.to_owned(), value),
        );
    }
    if let Some(shortcuts) = legacy
        .and_then(|legacy| legacy.get(LEGACY_SHORTCUTS))
        .and_then(Value::as_object)
    {
        for (id, value) in shortcuts {
            derived.insert(
                shortcut_path(id),
                (SHORTCUTS, id.clone(), Some(value.clone())),
            );
        }
    }
    derived
}

/// The normalized value of one derived path: the value if it passes its
/// grammar, else the scalar default; `None` for an absent or invalid shortcut.
fn normalize_one(path: &str, value: Option<&Value>) -> Option<Value> {
    if let Some(id) = path.strip_prefix("keyboardShortcuts.") {
        return value
            .filter(|value| grammar::is_shortcut_id(id) && grammar::is_shortcut_value(value))
            .cloned();
    }
    let scalar = SCALARS.iter().find(|scalar| scalar.path() == path)?;
    Some(match value {
        Some(value) if scalar.accepts(value) => value.clone(),
        _ => (scalar.default)(),
    })
}

/// BLAKE3 over the normalized derived values, serialized with sorted keys at
/// every level. Raw bytes are never hashed: Engine reformats and migrates the
/// file without changing these values.
fn fingerprint(derived: &Derived) -> String {
    let mut normalized = Map::new();
    for (path, (_, _, value)) in derived {
        if let Some(value) = normalize_one(path, value.as_ref()) {
            normalized.insert(path.clone(), canonical(&value));
        }
    }
    let bytes = serde_json::to_vec(&Value::Object(normalized)).unwrap_or_default();
    blake3::hash(&bytes).to_hex().to_string()
}

/// A copy with object keys inserted in sorted order, so serialization is
/// canonical whatever the map's iteration order.
fn canonical(value: &Value) -> Value {
    match value {
        Value::Object(object) => {
            let sorted: BTreeMap<&String, &Value> = object.iter().collect();
            Value::Object(
                sorted
                    .into_iter()
                    .map(|(key, value)| (key.clone(), canonical(value)))
                    .collect(),
            )
        }
        Value::Array(items) => Value::Array(items.iter().map(canonical).collect()),
        other => other.clone(),
    }
}

fn write_marker(document: &mut Map<String, Value>, fingerprint: Option<&str>) {
    // `None` keeps an existing fingerprint.
    document
        .entry("version")
        .or_insert_with(|| Value::from(FORMAT_VERSION));
    let mut import = take_object(document, "import");
    import.insert("fieldSet".to_owned(), Value::from(CURRENT_FIELD_SET));
    if let Some(fingerprint) = fingerprint {
        import.insert(
            "sourceFingerprint".to_owned(),
            Value::String(fingerprint.to_owned()),
        );
    }
    document.insert("import".to_owned(), Value::Object(import));
}

fn persist(document: &Map<String, Value>, product_path: &Path) -> io::Result<()> {
    let bytes = serde_json::to_vec_pretty(document).map_err(io::Error::other)?;
    write_atomic(product_path, &bytes)
}

fn defer(reason: &'static str) -> ImportOutcome {
    warn!(
        error_kind = "legacy_settings_unreadable",
        reason, "Legacy settings unreadable; import marker not advanced, retried on the next start"
    );
    ImportOutcome::Deferred(reason)
}

fn write_failed(error: io::Error) -> ImportOutcome {
    warn!(
        error = %error,
        error_kind = "desktop_settings_import_write_failed",
        "Product settings import could not be written; retried on the next start"
    );
    ImportOutcome::WriteFailed(error)
}
