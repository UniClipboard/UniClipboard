//! Helpers for normalizing Windows CF_HTML payloads.
//!
//! `clipboard_win::raw::set_html` unconditionally prepends a
//! `<html>\r\n<body>\r\n<!--StartFragment-->` header and appends a
//! `<!--EndFragment-->\r\n</body>\r\n</html>` footer around whatever it is
//! handed. On the read side, `clipboard-rs::get_html` returns the full
//! document delimited by `StartHTML..EndHTML`, i.e. **including** those
//! wrappers. Without normalization, every Win → peer → Win round-trip nests
//! one extra wrapper layer; the user's `content_hash`-based dedup cannot
//! collapse them because each layer changes the hash.
//!
//! Lives outside the `windows.rs` `cfg` gate so unit tests run on every host.

/// Strip every outer CF_HTML wrapper introduced by `clipboard-win::raw::set_html`,
/// returning the inner fragment payload.
///
/// Detection is anchored on the literal `<!--StartFragment-->` /
/// `<!--EndFragment-->` markers, which are unique to CF_HTML and never appear
/// in HTML that a normal source application emits. The function picks the
/// **innermost** `StartFragment` (`rfind`, i.e. rightmost) and then the
/// nearest following `EndFragment`, so a payload that has already accumulated
/// N nested layers is collapsed back to the original fragment in a single
/// call.
///
/// Why `rfind` for `StartFragment`: in an N-layer nesting all N `StartFragment`
/// markers appear before all N `EndFragment` markers in source order, so a
/// naive `find`/`find` pair would span the outermost-Start to the innermost-End
/// and leave N-1 layers of opening wrappers inside the result. The
/// innermost-Start to its first-following-End is the only balanced pair.
pub(crate) fn strip_cf_html_wrapper(html: &str) -> &str {
    const START_MARKER: &str = "<!--StartFragment-->";
    const END_MARKER: &str = "<!--EndFragment-->";

    let Some(start_idx) = html.rfind(START_MARKER) else {
        return html;
    };
    let fragment_start = start_idx + START_MARKER.len();
    let Some(end_offset) = html[fragment_start..].find(END_MARKER) else {
        return html;
    };
    &html[fragment_start..fragment_start + end_offset]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passthrough_when_no_markers() {
        let html = "<p>plain web html with no CF_HTML markers</p>";
        assert_eq!(strip_cf_html_wrapper(html), html);
    }

    #[test]
    fn extracts_fragment_from_one_wrapper() {
        let html = "<html>\r\n<body>\r\n<!--StartFragment--><p>hello</p><!--EndFragment-->\r\n</body>\r\n</html>";
        assert_eq!(strip_cf_html_wrapper(html), "<p>hello</p>");
    }

    #[test]
    fn collapses_nested_wrappers_in_one_call() {
        // Reproduces the user's observed pathological state: 4 nested layers
        // around a single fragment. A single normalize call must collapse
        // back to the innermost payload.
        let mut current = String::from("<p>inner payload</p>");
        for _ in 0..4 {
            current = format!(
                "<html>\r\n<body>\r\n<!--StartFragment-->{current}<!--EndFragment-->\r\n</body>\r\n</html>"
            );
        }
        assert_eq!(strip_cf_html_wrapper(&current), "<p>inner payload</p>");
    }

    #[test]
    fn returns_input_when_only_start_marker_present() {
        // Defensive: a malformed CF_HTML buffer with only StartFragment must
        // not panic and must not silently truncate.
        let html = "<html><body><!--StartFragment--><p>broken</p></body></html>";
        assert_eq!(strip_cf_html_wrapper(html), html);
    }

    #[test]
    fn returns_input_when_only_end_marker_present() {
        let html = "<html><body><p>broken</p><!--EndFragment--></body></html>";
        assert_eq!(strip_cf_html_wrapper(html), html);
    }

    #[test]
    fn preserves_meta_and_attributes_inside_fragment() {
        // Matches the user's reproduction: a meta tag with attributes lives
        // inside the innermost fragment and must survive normalization.
        let html = "<html>\r\n<body>\r\n<!--StartFragment--><meta http-equiv=\"content-type\" content=\"text/html; charset=utf-8\">UniClipboard is the open-source clipboard.<!--EndFragment-->\r\n</body>\r\n</html>";
        assert_eq!(
            strip_cf_html_wrapper(html),
            "<meta http-equiv=\"content-type\" content=\"text/html; charset=utf-8\">UniClipboard is the open-source clipboard."
        );
    }

    #[test]
    fn handles_empty_fragment() {
        let html = "<html><body><!--StartFragment--><!--EndFragment--></body></html>";
        assert_eq!(strip_cf_html_wrapper(html), "");
    }
}
