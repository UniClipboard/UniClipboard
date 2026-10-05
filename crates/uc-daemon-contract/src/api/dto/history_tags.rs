//! Shared transport DTOs for the local history tag endpoints (`/history/tags`).
//!
//! Tags and their entry associations live only in this device's history: the
//! Engine never puts them in a sync payload. How tags are presented — each
//! tag's color and which tags the sidebar shows, in what order — is the
//! daemon's own tag layout (`/history/tags/layout`), not part of the Engine's
//! tag.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// One local history tag. `name` is `null` when the stored name cannot be
/// decrypted; such a tag can only be deleted.
#[derive(Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct HistoryTagDto {
    pub tag_id: String,
    pub name: Option<String>,
    pub created_at_ms: i64,
    /// Associations in the authoritative table. `/search/tags` counts indexed
    /// entries instead, so the two may briefly differ while the index rebuilds.
    pub entry_count: u32,
}

impl fmt::Debug for HistoryTagDto {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HistoryTagDto")
            .field("tag_id", &self.tag_id)
            .field("has_name", &self.name.is_some())
            .field("entry_count", &self.entry_count)
            .finish_non_exhaustive()
    }
}

/// `POST /history/tags` body. `color` applies only when the tag is new; an
/// existing tag returned by create-or-get keeps its color.
#[derive(Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateHistoryTagRequest {
    pub name: String,
    #[serde(default)]
    pub color: Option<HistoryTagColorDto>,
}

impl fmt::Debug for CreateHistoryTagRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CreateHistoryTagRequest")
            .finish_non_exhaustive()
    }
}

/// Create-or-get result: `created` is `false` when a tag with the same
/// normalized, case-insensitive name already existed and `tag` is that tag.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct HistoryTagCreatedDto {
    pub tag: HistoryTagDto,
    pub created: bool,
}

/// `PATCH /history/tags/{tag_id}` body.
#[derive(Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RenameHistoryTagRequest {
    pub name: String,
}

impl fmt::Debug for RenameHistoryTagRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RenameHistoryTagRequest")
            .finish_non_exhaustive()
    }
}

/// Rename outcome. A name taken by another tag is not written; the GUI offers
/// to merge into `existingTagId` instead.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HistoryTagRenameResultDto {
    Renamed {
        tag: HistoryTagDto,
    },
    NameConflict {
        #[serde(rename = "existingTagId")]
        existing_tag_id: String,
    },
}

/// Entry ids for add/remove and for the selection summary (1..=1000 per call;
/// duplicates are collapsed).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct HistoryTagEntriesRequest {
    pub entry_ids: Vec<String>,
}

/// Add/remove outcome. Pairs already in the requested state count as
/// `unchanged`; ids of missing entries are skipped and listed, and the existing
/// entries are applied in one transaction.
#[derive(Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct HistoryTagBatchResultDto {
    pub changed: u32,
    pub unchanged: u32,
    pub missing_entry_ids: Vec<String>,
}

impl fmt::Debug for HistoryTagBatchResultDto {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HistoryTagBatchResultDto")
            .field("changed", &self.changed)
            .field("unchanged", &self.unchanged)
            .field("missing_count", &self.missing_entry_ids.len())
            .finish()
    }
}

/// How many of the selected entries carry one tag.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct HistoryTagApplicationDto {
    pub tag_id: String,
    pub applied: u32,
}

/// Tags on a selection: `applied == selected` means every selected entry has
/// the tag; `0 < applied < selected` is a partial tag. `selected` counts only
/// entries that still exist.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntryTagSummaryDto {
    pub selected: u32,
    pub tags: Vec<HistoryTagApplicationDto>,
}

/// `POST /history/tags/{tag_id}/merge` body: tags folded into the path tag,
/// then deleted (1..=100; the target must not be among them).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MergeHistoryTagsRequest {
    pub source_tag_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct HistoryTagMergeResultDto {
    pub moved: u32,
    pub already_on_target: u32,
}

/// Deleting a tag only detaches it; the entries stay.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct HistoryTagDeletedDto {
    pub detached: u32,
}

/// A tag color: one of the five palette colors (HDetail.dc.html), or a custom
/// sRGB color. On the wire it is the palette name (`"orange"`) or `#rrggbb`
/// in lowercase; nothing else parses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HistoryTagColorDto {
    Orange,
    Blue,
    Green,
    Purple,
    Gray,
    Custom([u8; 3]),
}

impl HistoryTagColorDto {
    /// Parse a palette name or a `#rrggbb` color (either case).
    pub fn parse(raw: &str) -> Option<Self> {
        Some(match raw {
            "orange" => Self::Orange,
            "blue" => Self::Blue,
            "green" => Self::Green,
            "purple" => Self::Purple,
            "gray" => Self::Gray,
            _ => {
                let hex = raw.strip_prefix('#').filter(|hex| hex.len() == 6)?;
                let channel = |at: usize| u8::from_str_radix(hex.get(at..at + 2)?, 16).ok();
                Self::Custom([channel(0)?, channel(2)?, channel(4)?])
            }
        })
    }
}

impl fmt::Display for HistoryTagColorDto {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Orange => formatter.write_str("orange"),
            Self::Blue => formatter.write_str("blue"),
            Self::Green => formatter.write_str("green"),
            Self::Purple => formatter.write_str("purple"),
            Self::Gray => formatter.write_str("gray"),
            Self::Custom([r, g, b]) => write!(formatter, "#{r:02x}{g:02x}{b:02x}"),
        }
    }
}

impl Serialize for HistoryTagColorDto {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for HistoryTagColorDto {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Self::parse(&raw).ok_or_else(|| serde::de::Error::custom("not a tag color"))
    }
}

impl<'s> ToSchema<'s> for HistoryTagColorDto {
    fn schema() -> (
        &'s str,
        utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>,
    ) {
        use utoipa::openapi::schema::{ObjectBuilder, SchemaType};
        (
            "HistoryTagColorDto",
            ObjectBuilder::new()
                .schema_type(SchemaType::String)
                .pattern(Some("^(orange|blue|green|purple|gray|#[0-9a-fA-F]{6})$"))
                .description(Some(
                    "A palette color name, or a custom color as `#rrggbb` (lowercase when returned).",
                ))
                .into(),
        )
    }
}

/// How tags are presented: the tags the sidebar shows, in order, and each
/// tag's color. Ids are builtin tag ids (`link`, `code`, `image`,
/// `directory`) or local tag ids; a local tag has a color only once one was
/// chosen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct HistoryTagLayoutDto {
    pub sidebar: Vec<String>,
    pub colors: BTreeMap<String, HistoryTagColorDto>,
}

/// `PUT /history/tags/layout/sidebar` body: the sidebar's tags, in order.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetHistoryTagSidebarRequest {
    pub tag_ids: Vec<String>,
}

/// `PUT /history/tags/{tag_id}/color` body; `null` clears a local tag's color
/// (a builtin tag goes back to its default).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetHistoryTagColorRequest {
    pub color: Option<HistoryTagColorDto>,
}

/// `PUT /history/tags/{tag_id}/sidebar` body. Adding a tag puts it last.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetHistoryTagInSidebarRequest {
    pub in_sidebar: bool,
}

#[cfg(test)]
mod layout_wire_tests {
    use super::*;

    #[test]
    fn layout_and_requests_use_camel_case_and_lowercase_colors() {
        let layout = HistoryTagLayoutDto {
            sidebar: vec!["link".into()],
            colors: BTreeMap::from([("link".to_string(), HistoryTagColorDto::Blue)]),
        };
        assert_eq!(
            serde_json::to_value(&layout).unwrap(),
            serde_json::json!({ "sidebar": ["link"], "colors": { "link": "blue" } })
        );
        let request: SetHistoryTagInSidebarRequest =
            serde_json::from_value(serde_json::json!({ "inSidebar": true })).unwrap();
        assert!(request.in_sidebar);
        assert!(serde_json::from_value::<SetHistoryTagInSidebarRequest>(
            serde_json::json!({ "in_sidebar": true })
        )
        .is_err());
        let sidebar: SetHistoryTagSidebarRequest =
            serde_json::from_value(serde_json::json!({ "tagIds": ["code"] })).unwrap();
        assert_eq!(sidebar.tag_ids, ["code"]);
    }

    #[test]
    fn create_request_color_is_optional() {
        let bare: CreateHistoryTagRequest =
            serde_json::from_value(serde_json::json!({ "name": "deploy" })).unwrap();
        assert_eq!(bare.color, None);
        let colored: CreateHistoryTagRequest =
            serde_json::from_value(serde_json::json!({ "name": "deploy", "color": "orange" }))
                .unwrap();
        assert_eq!(colored.color, Some(HistoryTagColorDto::Orange));
        assert!(serde_json::from_value::<CreateHistoryTagRequest>(
            serde_json::json!({ "name": "deploy", "color": "red" })
        )
        .is_err());
    }

    #[test]
    fn colors_are_palette_names_or_lowercase_rrggbb() {
        let custom: HistoryTagColorDto =
            serde_json::from_value(serde_json::json!("#3E5FA8")).unwrap();
        assert_eq!(custom, HistoryTagColorDto::Custom([0x3e, 0x5f, 0xa8]));
        assert_eq!(
            serde_json::to_value(custom).unwrap(),
            serde_json::json!("#3e5fa8")
        );
        assert_eq!(
            serde_json::to_value(HistoryTagColorDto::Gray).unwrap(),
            serde_json::json!("gray")
        );
        for raw in [
            "red",
            "#fff",
            "#12345g",
            "3e5fa8",
            "#3e5fa8ff",
            "Orange",
            "",
        ] {
            assert!(HistoryTagColorDto::parse(raw).is_none(), "{raw}");
        }
    }
}
