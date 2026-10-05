//! The daemon's tag layout: which tags the History sidebar shows, in what
//! order, and each tag's color.
//!
//! The Engine owns tags, their (sealed) names and their entry associations;
//! how they are presented is product state owned here. The layout is stored
//! as plain JSON by explicit approval (`.planning/history-tag-metadata/plan.md`):
//! it holds nothing but tag ids — builtin ids or Engine-generated UUIDs — a
//! color (a palette name or `#rrggbb`) and an order. [`TagLayoutId`] and
//! [`HistoryTagColorDto`] refuse anything else, so no user content can reach
//! the file; adding a free-text field reopens that decision.

use std::collections::{BTreeMap, HashSet};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Deserializer, Serialize};
use tokio::sync::Mutex;
use tracing::warn;
use uc_daemon_contract::api::dto::history_tags::{HistoryTagColorDto, HistoryTagLayoutDto};

/// Builtin tags a layout can hold, with their default colors, in their default
/// sidebar order. `favorited` is left out: the Library's Pinned row is that tag.
const BUILTINS: [(&str, HistoryTagColorDto); 4] = [
    ("link", HistoryTagColorDto::Blue),
    ("code", HistoryTagColorDto::Purple),
    ("image", HistoryTagColorDto::Green),
    ("directory", HistoryTagColorDto::Gray),
];

/// Largest layout file read back; a layout is ids and enums, far below this.
const MAX_BYTES: u64 = 1024 * 1024;

/// A tag id the layout may hold: a builtin id from [`BUILTINS`], or a
/// hyphenated UUID (the Engine's local tag ids). Nothing else parses.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct TagLayoutId(String);

impl TagLayoutId {
    pub fn parse(raw: &str) -> Option<Self> {
        let builtin = BUILTINS.iter().any(|(id, _)| *id == raw);
        let uuid = raw.len() == 36 && uuid::Uuid::try_parse(raw).is_ok();
        (builtin || uuid).then(|| Self(raw.to_owned()))
    }

    pub fn is_builtin(&self) -> bool {
        BUILTINS.iter().any(|(id, _)| *id == self.0)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for TagLayoutId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Self::parse(&raw).ok_or_else(|| serde::de::Error::custom("not a tag layout id"))
    }
}

/// The stored layout. `colors` holds only colors someone chose; builtin
/// defaults are added when it is read.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Document {
    version: u32,
    sidebar: Vec<TagLayoutId>,
    colors: BTreeMap<TagLayoutId, HistoryTagColorDto>,
}

impl Default for Document {
    fn default() -> Self {
        Self {
            version: 1,
            sidebar: BUILTINS
                .iter()
                .map(|(id, _)| TagLayoutId(id.to_string()))
                .collect(),
            colors: BTreeMap::new(),
        }
    }
}

impl Document {
    fn to_dto(&self) -> HistoryTagLayoutDto {
        let mut colors: BTreeMap<String, HistoryTagColorDto> = BUILTINS
            .iter()
            .map(|(id, color)| (id.to_string(), *color))
            .collect();
        for (id, color) in &self.colors {
            colors.insert(id.0.clone(), *color);
        }
        HistoryTagLayoutDto {
            sidebar: self.sidebar.iter().map(|id| id.0.clone()).collect(),
            colors,
        }
    }

    /// Drop local tags the Engine no longer has (deleted elsewhere, or the
    /// space was reset). Builtin ids always stay.
    fn prune(&mut self, local: &HashSet<String>) {
        let keep = |id: &TagLayoutId| id.is_builtin() || local.contains(&id.0);
        self.sidebar.retain(keep);
        self.colors.retain(|id, _| keep(id));
    }

    fn set_in_sidebar(&mut self, id: &TagLayoutId, in_sidebar: bool) {
        let present = self.sidebar.contains(id);
        if in_sidebar && !present {
            self.sidebar.push(id.clone());
        } else if !in_sidebar {
            self.sidebar.retain(|other| other != id);
        }
    }

    fn set_sidebar(&mut self, ids: Vec<TagLayoutId>) {
        let mut seen = HashSet::new();
        self.sidebar = ids
            .into_iter()
            .filter(|id| seen.insert(id.clone()))
            .collect();
    }

    fn forget(&mut self, ids: &[TagLayoutId]) {
        self.sidebar.retain(|id| !ids.contains(id));
        self.colors.retain(|id, _| !ids.contains(id));
    }

    /// After `sources` were folded into `target`: the target takes the first
    /// source's sidebar slot when it had none, and a source's color when it
    /// had none; the sources go.
    fn merge(&mut self, target: &TagLayoutId, sources: &[TagLayoutId]) {
        if !self.sidebar.contains(target) {
            if let Some(slot) = self.sidebar.iter().position(|id| sources.contains(id)) {
                self.sidebar[slot] = target.clone();
            }
        }
        if !self.colors.contains_key(target) {
            if let Some(color) = sources.iter().find_map(|id| self.colors.get(id).copied()) {
                self.colors.insert(target.clone(), color);
            }
        }
        self.forget(sources);
    }
}

/// The one owner of the tag layout: an in-memory copy, written through to
/// its file on every change. Without a file (tests, assembly paths that do
/// not wire one) it lives in memory only.
pub struct TagLayoutStore {
    path: Option<PathBuf>,
    document: Mutex<Document>,
}

impl TagLayoutStore {
    pub fn in_memory() -> Self {
        Self {
            path: None,
            document: Mutex::new(Document::default()),
        }
    }

    /// Load the layout at `path`. A missing file is the default layout; an
    /// unreadable one is logged and replaced by the default on the next change.
    pub fn load(path: PathBuf) -> Self {
        let document = match read(&path) {
            Ok(document) => document,
            Err(error) => {
                warn!(error_kind = "tag_layout_read", error = %error, "Tag layout unreadable; using the default layout");
                Document::default()
            }
        };
        Self {
            path: Some(path),
            document: Mutex::new(document),
        }
    }

    /// The layout without local tags missing from `local` (the Engine's tag ids).
    pub async fn snapshot(&self, local: &HashSet<String>) -> io::Result<HistoryTagLayoutDto> {
        self.change(|document| document.prune(local)).await
    }

    pub async fn set_color(
        &self,
        id: &TagLayoutId,
        color: Option<HistoryTagColorDto>,
    ) -> io::Result<HistoryTagLayoutDto> {
        self.change(|document| match color {
            Some(color) => {
                document.colors.insert(id.clone(), color);
            }
            None => {
                document.colors.remove(id);
            }
        })
        .await
    }

    pub async fn set_in_sidebar(
        &self,
        id: &TagLayoutId,
        in_sidebar: bool,
    ) -> io::Result<HistoryTagLayoutDto> {
        self.change(|document| document.set_in_sidebar(id, in_sidebar))
            .await
    }

    pub async fn set_sidebar(&self, ids: Vec<TagLayoutId>) -> io::Result<HistoryTagLayoutDto> {
        self.change(|document| document.set_sidebar(ids)).await
    }

    pub async fn forget(&self, ids: &[TagLayoutId]) -> io::Result<HistoryTagLayoutDto> {
        self.change(|document| document.forget(ids)).await
    }

    pub async fn merge(
        &self,
        target: &TagLayoutId,
        sources: &[TagLayoutId],
    ) -> io::Result<HistoryTagLayoutDto> {
        self.change(|document| document.merge(target, sources))
            .await
    }

    /// Apply `edit`, write the result when it changed anything, and only then
    /// keep it: a failed write leaves the layout as it was.
    async fn change(&self, edit: impl FnOnce(&mut Document)) -> io::Result<HistoryTagLayoutDto> {
        let mut document = self.document.lock().await;
        let mut next = document.clone();
        edit(&mut next);
        if next != *document {
            if let Some(path) = &self.path {
                let path = path.clone();
                let bytes = serde_json::to_vec_pretty(&next)?;
                tokio::task::spawn_blocking(move || write_atomic(&path, &bytes))
                    .await
                    .map_err(io::Error::other)??;
            }
            *document = next;
        }
        Ok(document.to_dto())
    }
}

fn read(path: &Path) -> io::Result<Document> {
    use std::io::Read;
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Document::default()),
        Err(error) => return Err(error),
    };
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(io::Error::other("tag layout too large"));
    }
    let document: Document = serde_json::from_slice(&bytes)?;
    if document.version != 1 {
        return Err(io::Error::other("unsupported tag layout version"));
    }
    Ok(document)
}

/// Write `bytes` to `path` through a synced temporary file in the same
/// directory, so a crash leaves either the old layout or the new one.
fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("missing tag layout directory"))?;
    std::fs::create_dir_all(parent)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(bytes)?;
    temporary.flush()?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|error| error.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "0b9e3c1a-5f2d-4c8e-9a7b-1d2e3f4a5b6c";
    const B: &str = "7c6d5e4f-3a2b-4c1d-8e9f-0a1b2c3d4e5f";

    fn id(raw: &str) -> TagLayoutId {
        TagLayoutId::parse(raw).unwrap()
    }

    fn local(ids: &[&str]) -> HashSet<String> {
        ids.iter().map(|id| id.to_string()).collect()
    }

    #[test]
    fn ids_are_builtin_tags_or_uuids_and_nothing_else() {
        for raw in ["link", "code", "image", "directory", A] {
            assert!(TagLayoutId::parse(raw).is_some(), "{raw}");
        }
        // `favorited` is the Pinned row; free text never parses.
        for raw in [
            "favorited",
            "file",
            "deploy",
            "my tag",
            "",
            "0b9e3c1a5f2d4c8e9a7b1d2e3f4a5b6c",
        ] {
            assert!(TagLayoutId::parse(raw).is_none(), "{raw}");
        }
        assert!(serde_json::from_value::<TagLayoutId>(serde_json::json!("deploy")).is_err());
    }

    #[test]
    fn a_stored_layout_with_anything_but_ids_is_refused() {
        let named = serde_json::json!({
            "version": 1, "sidebar": ["link"], "colors": {}, "names": { "link": "x" }
        });
        assert!(serde_json::from_value::<Document>(named).is_err());
        let free_text =
            serde_json::json!({ "version": 1, "sidebar": ["release notes"], "colors": {} });
        assert!(serde_json::from_value::<Document>(free_text).is_err());
    }

    #[tokio::test]
    async fn the_default_layout_shows_four_builtins_with_their_colors() {
        let layout = TagLayoutStore::in_memory()
            .snapshot(&local(&[]))
            .await
            .unwrap();
        assert_eq!(layout.sidebar, ["link", "code", "image", "directory"]);
        assert_eq!(layout.colors.get("link"), Some(&HistoryTagColorDto::Blue));
        assert_eq!(layout.colors.len(), 4);
    }

    #[tokio::test]
    async fn sidebar_changes_append_remove_and_reorder_without_duplicates() {
        let store = TagLayoutStore::in_memory();
        store.set_in_sidebar(&id(A), true).await.unwrap();
        store.set_in_sidebar(&id(A), true).await.unwrap();
        let layout = store.set_in_sidebar(&id("code"), false).await.unwrap();
        assert_eq!(layout.sidebar, ["link", "image", "directory", A]);

        let layout = store
            .set_sidebar(vec![id(A), id("link"), id(A)])
            .await
            .unwrap();
        assert_eq!(layout.sidebar, [A, "link"]);
    }

    #[tokio::test]
    async fn colors_override_and_reset_builtin_defaults() {
        let store = TagLayoutStore::in_memory();
        let layout = store
            .set_color(&id("link"), Some(HistoryTagColorDto::Orange))
            .await
            .unwrap();
        assert_eq!(layout.colors.get("link"), Some(&HistoryTagColorDto::Orange));
        let layout = store.set_color(&id("link"), None).await.unwrap();
        assert_eq!(layout.colors.get("link"), Some(&HistoryTagColorDto::Blue));
    }

    #[tokio::test]
    async fn listing_drops_local_tags_the_engine_no_longer_has() {
        let store = TagLayoutStore::in_memory();
        store.set_in_sidebar(&id(A), true).await.unwrap();
        store
            .set_color(&id(B), Some(HistoryTagColorDto::Green))
            .await
            .unwrap();
        let layout = store.snapshot(&local(&[A])).await.unwrap();
        assert_eq!(layout.sidebar, ["link", "code", "image", "directory", A]);
        assert!(!layout.colors.contains_key(B));
    }

    #[tokio::test]
    async fn a_merge_target_inherits_the_sources_slot_and_color() {
        let store = TagLayoutStore::in_memory();
        store.set_sidebar(vec![id("link"), id(B)]).await.unwrap();
        store
            .set_color(&id(B), Some(HistoryTagColorDto::Purple))
            .await
            .unwrap();
        let layout = store.merge(&id(A), &[id(B)]).await.unwrap();
        assert_eq!(layout.sidebar, ["link", A]);
        assert_eq!(layout.colors.get(A), Some(&HistoryTagColorDto::Purple));
        assert!(!layout.colors.contains_key(B));
    }

    #[tokio::test]
    async fn changes_persist_and_reload_from_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history-tags").join("layout.json");
        let store = TagLayoutStore::load(path.clone());
        store
            .set_color(&id(A), Some(HistoryTagColorDto::Orange))
            .await
            .unwrap();
        store.set_in_sidebar(&id(A), true).await.unwrap();

        let reloaded = TagLayoutStore::load(path.clone());
        let layout = reloaded.snapshot(&local(&[A])).await.unwrap();
        assert_eq!(layout.sidebar, ["link", "code", "image", "directory", A]);
        assert_eq!(layout.colors.get(A), Some(&HistoryTagColorDto::Orange));
        let stored: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(stored["colors"], serde_json::json!({ A: "orange" }));
    }

    #[tokio::test]
    async fn a_custom_color_is_stored_as_lowercase_rrggbb() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("layout.json");
        let store = TagLayoutStore::load(path.clone());
        let color = HistoryTagColorDto::parse("#3E5FA8").unwrap();
        store.set_color(&id(A), Some(color)).await.unwrap();

        let stored: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(stored["colors"], serde_json::json!({ A: "#3e5fa8" }));
        let layout = TagLayoutStore::load(path)
            .snapshot(&local(&[A]))
            .await
            .unwrap();
        assert_eq!(layout.colors.get(A), Some(&color));
    }

    #[tokio::test]
    async fn an_unreadable_file_falls_back_to_the_default_until_the_next_change() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("layout.json");
        std::fs::write(&path, b"{ not json").unwrap();
        let store = TagLayoutStore::load(path.clone());
        let layout = store.snapshot(&local(&[])).await.unwrap();
        assert_eq!(layout.sidebar, ["link", "code", "image", "directory"]);
        // Reading alone leaves the file untouched.
        assert_eq!(std::fs::read(&path).unwrap(), b"{ not json");
        store.set_in_sidebar(&id("code"), false).await.unwrap();
        assert!(serde_json::from_slice::<Document>(&std::fs::read(&path).unwrap()).is_ok());
    }
}
