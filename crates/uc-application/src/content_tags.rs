//! Shared application-layer evaluation for builtin content tags.

use uc_core::clipboard::link_utils::detect_link_urls;
use uc_core::search::tag::{TagId, TagRule, TaggableContent};

/// A [`TagRule`] that marks content carrying one or more web URLs with the
/// builtin `link` tag. The membership decision and the `linkUrls` render
/// metadata share [`detect_link_urls`], so they stay in lock-step.
struct LinkRule {
    tag_id: TagId,
}

impl LinkRule {
    fn new() -> Self {
        Self {
            tag_id: TagId::link(),
        }
    }
}

impl TagRule for LinkRule {
    fn tag_id(&self) -> &TagId {
        &self.tag_id
    }

    fn evaluate(&self, content: &TaggableContent<'_>) -> bool {
        !detect_link_urls(content.uri_list, content.plain_text).is_empty()
    }
}

/// A [`TagRule`] that marks entries carrying image content with the builtin
/// `image` tag.
///
/// Unlike `content_type` — which faithfully reflects the physical entry
/// category — this tag answers "does this entry contain an image?". It therefore
/// surfaces both pure bitmaps and image files under the image filter, mirroring
/// the way the `link` tag is orthogonal to the physical content type.
struct ImageRule {
    tag_id: TagId,
}

impl ImageRule {
    fn new() -> Self {
        Self {
            tag_id: TagId::image(),
        }
    }
}

impl TagRule for ImageRule {
    fn tag_id(&self) -> &TagId {
        &self.tag_id
    }

    fn evaluate(&self, content: &TaggableContent<'_>) -> bool {
        content.has_image
    }
}

/// A [`TagRule`] that marks file entries containing a copied directory root.
struct DirectoryRule {
    tag_id: TagId,
}

impl DirectoryRule {
    fn new() -> Self {
        Self {
            tag_id: TagId::directory(),
        }
    }
}

impl TagRule for DirectoryRule {
    fn tag_id(&self) -> &TagId {
        &self.tag_id
    }

    fn evaluate(&self, content: &TaggableContent<'_>) -> bool {
        content.has_directory
    }
}

/// A [`TagRule`] that marks plain-text snippets that look like source code with
/// the builtin `code` tag.
struct CodeRule {
    tag_id: TagId,
}

impl CodeRule {
    fn new() -> Self {
        Self {
            tag_id: TagId::code(),
        }
    }
}

impl TagRule for CodeRule {
    fn tag_id(&self) -> &TagId {
        &self.tag_id
    }

    fn evaluate(&self, content: &TaggableContent<'_>) -> bool {
        looks_like_code(content.plain_text)
    }
}

fn looks_like_code(text: Option<&str>) -> bool {
    let Some(text) = text else {
        return false;
    };
    let trimmed = text.trim();
    if trimmed.len() < 12 {
        return false;
    }

    let lines: Vec<&str> = trimmed.lines().take(12).collect();
    // Keep this list free of words that appear in ordinary prose ("return",
    // "from", …): a single common word must not be enough to tag a note as code.
    let has_code_keyword = [
        "function ",
        "const ",
        "interface ",
        "import ",
        "export ",
        "def ",
        "fn ",
        "impl ",
        "struct ",
        "func ",
        "package ",
        "SELECT ",
        "INSERT INTO ",
        "UPDATE ",
        "DELETE FROM ",
        "CREATE TABLE ",
    ]
    .iter()
    .any(|keyword| trimmed.contains(keyword));
    let has_code_punctuation = trimmed.contains('{')
        || trimmed.contains('}')
        || trimmed.contains("=>")
        || trimmed.contains("->")
        || trimmed.contains("::")
        || trimmed.contains("</")
        || trimmed.contains("/>")
        || trimmed.contains("#include");
    let indented_lines = lines
        .iter()
        .filter(|line| line.starts_with("  ") || line.starts_with('\t'))
        .count();
    // `": "` is intentionally excluded — it is far more common in prose
    // ("Notes: …") than the punctuation/operators below that signal real code.
    let assignment_like = trimmed.contains(" = ")
        || trimmed.contains(" := ")
        || trimmed.contains("==")
        || trimmed.contains("!=");
    let comment_like = lines.iter().any(|line| {
        let s = line.trim_start();
        s.starts_with("//") || s.starts_with("/*") || s.starts_with("# ") || s.starts_with("-- ")
    });

    (has_code_keyword && (has_code_punctuation || assignment_like || indented_lines > 0))
        || (has_code_punctuation && indented_lines > 0)
        || (comment_like && (has_code_keyword || has_code_punctuation))
}

/// The builtin tag rules evaluated for every entry: [`LinkRule`] (web URLs),
/// [`ImageRule`] (image content), [`DirectoryRule`] (directory roots), and
/// [`CodeRule`] (source-like text).
/// User-defined rules are a later extension point.
fn builtin_rules() -> Vec<Box<dyn TagRule>> {
    vec![
        Box::new(LinkRule::new()),
        Box::new(ImageRule::new()),
        Box::new(DirectoryRule::new()),
        Box::new(CodeRule::new()),
    ]
}

/// Evaluate builtin content rules, collecting the ids of the tags that apply.
pub(crate) fn evaluate_builtin_content_tags(content: &TaggableContent<'_>) -> Vec<TagId> {
    builtin_rules()
        .iter()
        .filter(|rule| rule.evaluate(content))
        .map(|rule| rule.tag_id().clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use uc_core::search::document::ContentType;

    fn tags_for(
        plain_text: Option<&str>,
        uri_list: &[String],
        has_image: bool,
        has_directory: bool,
    ) -> Vec<TagId> {
        evaluate_builtin_content_tags(&TaggableContent {
            content_type: ContentType::Text,
            uri_list,
            plain_text,
            has_image,
            has_directory,
        })
    }

    #[test]
    fn plain_text_url_gets_link_tag() {
        assert_eq!(
            tags_for(Some("https://example.com"), &[], false, false),
            vec![TagId::link()]
        );
    }

    #[test]
    fn plain_text_code_snippet_gets_code_tag() {
        let tags = tags_for(
            Some("function greet(name) {\n  return `hello ${name}`;\n}"),
            &[],
            false,
            false,
        );
        assert!(tags.contains(&TagId::code()));
    }

    #[test]
    fn prose_with_programming_words_has_no_code_tag() {
        let tags = tags_for(
            Some("Notes from today: please return the signed form after the meeting."),
            &[],
            false,
            false,
        );
        assert!(!tags.contains(&TagId::code()));
    }

    #[test]
    fn image_content_gets_image_tag() {
        let tags = tags_for(None, &[], true, false);
        assert_eq!(tags, vec![TagId::image()]);
    }

    #[test]
    fn directory_content_gets_directory_tag() {
        let tags = tags_for(None, &[], false, true);
        assert_eq!(tags, vec![TagId::directory()]);
    }
}
