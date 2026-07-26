use std::fmt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct FormatId(String);

impl FormatId {
    pub fn from_str(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for FormatId {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for FormatId {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

impl std::ops::Deref for FormatId {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl fmt::Display for FormatId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct RepresentationId(String);

impl RepresentationId {
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct MimeType(pub String);

impl MimeType {
    pub fn text_plain() -> Self {
        Self("text/plain".into())
    }

    pub fn text_html() -> Self {
        Self("text/html".into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn essence(&self) -> String {
        self.0
            .split(';')
            .next()
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase()
    }

    pub fn classify(&self) -> MimeClass {
        match self.essence().as_str() {
            "text/plain" => MimeClass::TextPlain,
            "text/html" => MimeClass::TextHtml,
            "text/rtf" | "application/rtf" => MimeClass::TextRtf,
            "text/markdown" => MimeClass::TextMarkdown,
            "text/uri-list" | "file/uri-list" => MimeClass::UriList,
            "text/x-uri" | "text/x-url" | "text/uri" => MimeClass::TextLink,
            "application/octet-stream" => MimeClass::OctetStream,
            "image/png" => MimeClass::Image(ImageKind::Png),
            "image/jpeg" | "image/jpg" => MimeClass::Image(ImageKind::Jpeg),
            "image/tiff" | "image/tif" => MimeClass::Image(ImageKind::Tiff),
            "image/gif" => MimeClass::Image(ImageKind::Gif),
            "image/webp" => MimeClass::Image(ImageKind::Webp),
            "image/bmp" | "image/x-bmp" => MimeClass::Image(ImageKind::Bmp),
            value if value.starts_with("image/") => MimeClass::Image(ImageKind::Other),
            value if value.starts_with("text/") => MimeClass::TextOther,
            _ => MimeClass::Unrecognized,
        }
    }

    pub fn is_text_plain(&self) -> bool {
        self.classify() == MimeClass::TextPlain
    }

    pub fn is_uri_list(&self) -> bool {
        self.classify() == MimeClass::UriList
    }

    pub fn is_image(&self) -> bool {
        matches!(self.classify(), MimeClass::Image(_))
    }
}

impl fmt::Display for MimeType {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::ops::Deref for MimeType {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MimeClass {
    TextPlain,
    TextHtml,
    TextRtf,
    TextMarkdown,
    UriList,
    TextLink,
    TextOther,
    Image(ImageKind),
    OctetStream,
    Unrecognized,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ImageKind {
    Png,
    Jpeg,
    Tiff,
    Gif,
    Webp,
    Bmp,
    Other,
}

#[derive(Debug, Clone)]
pub enum ClipboardPayloadSource {
    Inline(Vec<u8>),
    LocalFile { path: PathBuf, size_bytes: u64 },
}

#[derive(Debug, Clone)]
pub struct ObservedClipboardRepresentation {
    pub id: RepresentationId,
    pub format_id: FormatId,
    pub mime: Option<MimeType>,
    source: ClipboardPayloadSource,
}

#[derive(Serialize, Deserialize)]
struct ObservedRepresentationProxy {
    id: RepresentationId,
    format_id: FormatId,
    mime: Option<MimeType>,
    bytes: Vec<u8>,
}

impl Serialize for ObservedClipboardRepresentation {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let bytes = match &self.source {
            ClipboardPayloadSource::Inline(bytes) => bytes.clone(),
            ClipboardPayloadSource::LocalFile { path, .. } => {
                return Err(serde::ser::Error::custom(format!(
                    "path-backed clipboard representation cannot be serialized: {}",
                    path.display()
                )));
            }
        };

        ObservedRepresentationProxy {
            id: self.id.clone(),
            format_id: self.format_id.clone(),
            mime: self.mime.clone(),
            bytes,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ObservedClipboardRepresentation {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let proxy = ObservedRepresentationProxy::deserialize(deserializer)?;
        Ok(Self::new(
            proxy.id,
            proxy.format_id,
            proxy.mime,
            proxy.bytes,
        ))
    }
}

impl ObservedClipboardRepresentation {
    pub fn new(
        id: RepresentationId,
        format_id: FormatId,
        mime: Option<MimeType>,
        bytes: Vec<u8>,
    ) -> Self {
        Self {
            id,
            format_id,
            mime,
            source: ClipboardPayloadSource::Inline(bytes),
        }
    }

    pub fn new_local_file(
        id: RepresentationId,
        format_id: FormatId,
        mime: Option<MimeType>,
        path: PathBuf,
        size_bytes: u64,
    ) -> Self {
        Self {
            id,
            format_id,
            mime,
            source: ClipboardPayloadSource::LocalFile { path, size_bytes },
        }
    }

    pub fn source(&self) -> &ClipboardPayloadSource {
        &self.source
    }

    pub fn expect_inline_bytes(&self) -> &[u8] {
        match &self.source {
            ClipboardPayloadSource::Inline(bytes) => bytes,
            ClipboardPayloadSource::LocalFile { .. } => {
                panic!("path-backed clipboard representation requires explicit file handling")
            }
        }
    }

    pub fn inline_bytes(&self) -> Option<&[u8]> {
        match &self.source {
            ClipboardPayloadSource::Inline(bytes) => Some(bytes),
            ClipboardPayloadSource::LocalFile { .. } => None,
        }
    }

    pub fn size_bytes(&self) -> i64 {
        match &self.source {
            ClipboardPayloadSource::Inline(bytes) => bytes.len() as i64,
            ClipboardPayloadSource::LocalFile { size_bytes, .. } => *size_bytes as i64,
        }
    }

    fn content_key(&self) -> Option<String> {
        let hash = match &self.source {
            ClipboardPayloadSource::Inline(bytes) => blake3::hash(bytes),
            ClipboardPayloadSource::LocalFile { path, .. } => {
                blake3::hash(&std::fs::read(path).ok()?)
            }
        };
        Some(hash.to_hex().to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemClipboardSnapshot {
    pub ts_ms: i64,
    pub representations: Vec<ObservedClipboardRepresentation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub file_content_digests: Vec<[u8; 32]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_set_v1_component: Option<[u8; 32]>,
}

impl SystemClipboardSnapshot {
    pub fn is_empty(&self) -> bool {
        self.representations.is_empty()
    }

    pub fn primary_image_inline_bytes(&self) -> Option<&[u8]> {
        self.representations
            .iter()
            .find(|representation| is_image_representation(representation))
            .and_then(ObservedClipboardRepresentation::inline_bytes)
    }

    pub fn primary_image_size_bytes(&self) -> Option<i64> {
        self.representations
            .iter()
            .find(|representation| is_image_representation(representation))
            .map(ObservedClipboardRepresentation::size_bytes)
    }

    pub fn meaningful_origin_key(&self) -> Option<String> {
        for (prefix, predicate) in [
            (
                "files",
                is_file_representation as fn(&ObservedClipboardRepresentation) -> bool,
            ),
            ("text", is_plain_text_representation),
            ("rich-text", is_text_representation),
            ("image", is_image_representation),
        ] {
            if let Some(representation) = self
                .representations
                .iter()
                .find(|representation| predicate(representation))
            {
                return Some(format!("{prefix}:{}", representation.content_key()?));
            }
        }
        None
    }

    pub fn total_size_bytes(&self) -> i64 {
        self.representations
            .iter()
            .map(ObservedClipboardRepresentation::size_bytes)
            .sum()
    }
}

pub trait SystemClipboard: Send + Sync {
    fn read_snapshot(&self) -> anyhow::Result<SystemClipboardSnapshot>;
    fn write_snapshot(&self, snapshot: SystemClipboardSnapshot) -> anyhow::Result<()>;
}

pub fn is_file_mime_or_format(mime: Option<&MimeType>, format_id: &FormatId) -> bool {
    mime.is_some_and(MimeType::is_uri_list)
        || format_id.eq_ignore_ascii_case("files")
        || format_id.eq_ignore_ascii_case("public.file-url")
        || format_id.to_ascii_lowercase().contains("uri-list")
}

fn is_file_representation(representation: &ObservedClipboardRepresentation) -> bool {
    is_file_mime_or_format(representation.mime.as_ref(), &representation.format_id)
}

fn is_plain_text_representation(representation: &ObservedClipboardRepresentation) -> bool {
    representation
        .mime
        .as_ref()
        .is_some_and(|mime| mime.classify() == MimeClass::TextPlain)
        || representation.format_id.eq_ignore_ascii_case("text")
}

fn is_text_representation(representation: &ObservedClipboardRepresentation) -> bool {
    representation.mime.as_ref().is_some_and(|mime| {
        matches!(
            mime.classify(),
            MimeClass::TextPlain
                | MimeClass::TextHtml
                | MimeClass::TextRtf
                | MimeClass::TextMarkdown
                | MimeClass::TextLink
                | MimeClass::TextOther
        )
    })
}

fn is_image_representation(representation: &ObservedClipboardRepresentation) -> bool {
    representation
        .mime
        .as_ref()
        .is_some_and(|mime| matches!(mime.classify(), MimeClass::Image(_)))
        || representation
            .format_id
            .to_ascii_lowercase()
            .contains("image")
}

pub fn select_paste_representation_index(snapshot: &SystemClipboardSnapshot) -> Option<usize> {
    snapshot
        .representations
        .iter()
        .enumerate()
        .filter(|(_, representation)| representation.size_bytes() > 0)
        .max_by_key(|(_, representation)| {
            if is_file_representation(representation) {
                100
            } else {
                match representation.mime.as_ref().map(MimeType::classify) {
                    Some(MimeClass::TextHtml | MimeClass::TextRtf) => 90,
                    Some(MimeClass::TextPlain | MimeClass::TextOther | MimeClass::TextMarkdown) => {
                        80
                    }
                    Some(MimeClass::Image(_))
                        if representation
                            .format_id
                            .eq_ignore_ascii_case("image-from-file") =>
                    {
                        30
                    }
                    Some(MimeClass::Image(_)) => 70,
                    Some(MimeClass::TextLink) => 60,
                    _ => 10,
                }
            }
        })
        .map(|(index, _)| index)
}

#[cfg(test)]
mod tests {
    use super::MimeType;

    #[test]
    fn mime_type_text_plain_predicate_accepts_parameters_and_rejects_images() {
        assert!(MimeType("text/plain;charset=utf-8".into()).is_text_plain());
        assert!(!MimeType("image/png".into()).is_text_plain());
    }
}
