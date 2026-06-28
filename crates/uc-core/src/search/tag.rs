//! Tag domain model — the derived, multi-valued classification dimension that
//! is orthogonal to the single-valued physical `ContentType`.
//!
//! A `ContentType` answers "what data form is this?" — exactly one value per
//! entry. A tag answers "does this content satisfy some rule, or carry some
//! user state?" — zero or more per entry. The two dimensions are independent.
//!
//! Builtin tag ids are reserved constants. Custom tag ids are opaque; their
//! human-readable definitions are held authoritatively elsewhere and are not
//! part of this model.

use serde::{Deserialize, Serialize};

use crate::search::document::ContentType;

/// Reserved ids for builtin tags.
///
/// These ids are a stable contract and must not change; persisted and wire
/// representations depend on them.
pub mod builtin {
    /// Web-URL link tag (uri-list entries or plain text that is itself a URL).
    pub const LINK: &str = "link";
    /// User-marked favorite state.
    pub const FAVORITED: &str = "favorited";
    /// Image-content tag: the entry is or contains an image — a pure bitmap, a
    /// copied image file, or a multi-file selection that includes one. This is
    /// orthogonal to `content_type`: a copied image file is physically a
    /// `File`, but still carries the `image` tag so the image filter surfaces
    /// it alongside pure bitmaps.
    pub const IMAGE: &str = "image";
    /// Code/rich-text tag: the entry carries rich text / HTML content.
    pub const CODE: &str = "code";
}

/// Stable identifier of a search tag.
///
/// Builtin tags use the reserved ids in [`builtin`]; custom tags use opaque
/// ids whose definitions are held authoritatively outside this model.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TagId(String);

impl TagId {
    /// Wrap an arbitrary id string.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// The builtin `link` tag id.
    pub fn link() -> Self {
        Self::new(builtin::LINK)
    }

    /// The builtin `favorited` tag id.
    pub fn favorited() -> Self {
        Self::new(builtin::FAVORITED)
    }

    /// The builtin `image` tag id.
    pub fn image() -> Self {
        Self::new(builtin::IMAGE)
    }

    /// The builtin `code` tag id.
    pub fn code() -> Self {
        Self::new(builtin::CODE)
    }

    /// Borrow the id as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// True when this id is one of the reserved builtin ids.
    pub fn is_builtin(&self) -> bool {
        matches!(
            self.0.as_str(),
            builtin::LINK | builtin::FAVORITED | builtin::IMAGE | builtin::CODE
        )
    }
}

impl std::fmt::Display for TagId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for TagId {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for TagId {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

/// Provenance of a tag — how its membership is produced and where its
/// definition lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TagKind {
    /// Membership derived by evaluating a builtin content rule.
    BuiltinRule,
    /// Membership reflects an explicit user state held authoritatively
    /// elsewhere; this dimension carries a mirror of it.
    UserState,
    /// Membership derived by evaluating a user-defined rule whose definition is
    /// held authoritatively elsewhere.
    CustomRule,
}

/// Domain-neutral input a [`TagRule`] evaluates to decide tag membership.
///
/// Carries only what rules need to classify content: the physical type and the
/// textual content from which web URLs are detected. Borrows from the caller's
/// buffers; it owns nothing.
pub struct TaggableContent<'a> {
    /// The entry's physical content type.
    pub content_type: ContentType,
    /// URI-list entries, if any (already split into individual URIs).
    pub uri_list: &'a [String],
    /// The entry's plain-text body, if any.
    pub plain_text: Option<&'a str>,
    /// True when the entry carries any image representation — a pure bitmap or
    /// an image file. Drives the builtin `image` tag, which is orthogonal to
    /// `content_type` (a copied image file is physically a `File`).
    pub has_image: bool,
}

/// A producer of exactly one tag: given content, decides whether the tag
/// applies.
///
/// Each rule is bound to a single [`TagId`] and is a pure predicate over
/// [`TaggableContent`] — evaluating it has no side effects and depends only on
/// the supplied content.
pub trait TagRule: Send + Sync {
    /// The id of the tag this rule produces.
    fn tag_id(&self) -> &TagId;

    /// True when `content` satisfies this rule (the tag applies).
    fn evaluate(&self, content: &TaggableContent<'_>) -> bool;
}

/// A tag and how many entries currently carry it — the unit of a tag listing
/// (e.g. for a filter sidebar). `count` is the number of distinct entries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchTagCount {
    pub tag_id: TagId,
    pub count: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_constructors_match_reserved_ids() {
        assert_eq!(TagId::link().as_str(), builtin::LINK);
        assert_eq!(TagId::favorited().as_str(), builtin::FAVORITED);
        assert_eq!(TagId::image().as_str(), builtin::IMAGE);
        assert_eq!(TagId::code().as_str(), builtin::CODE);
        assert!(TagId::link().is_builtin());
        assert!(TagId::favorited().is_builtin());
        assert!(TagId::image().is_builtin());
        assert!(TagId::code().is_builtin());
    }

    #[test]
    fn custom_id_is_not_builtin() {
        let custom = TagId::new("4f1c2e0a-custom");
        assert!(!custom.is_builtin());
        assert_eq!(custom.as_str(), "4f1c2e0a-custom");
    }

    #[test]
    fn tag_id_serializes_as_a_bare_string() {
        let json = serde_json::to_string(&TagId::link()).expect("serialize");
        assert_eq!(
            json, "\"link\"",
            "TagId is a transparent string on the wire"
        );
        let back: TagId = serde_json::from_str("\"favorited\"").expect("deserialize");
        assert_eq!(back, TagId::favorited());
    }

    #[test]
    fn tag_kind_serializes_snake_case() {
        assert_eq!(
            serde_json::to_string(&TagKind::BuiltinRule).expect("serialize"),
            "\"builtin_rule\""
        );
        assert_eq!(
            serde_json::to_string(&TagKind::UserState).expect("serialize"),
            "\"user_state\""
        );
        assert_eq!(
            serde_json::to_string(&TagKind::CustomRule).expect("serialize"),
            "\"custom_rule\""
        );
    }
}
