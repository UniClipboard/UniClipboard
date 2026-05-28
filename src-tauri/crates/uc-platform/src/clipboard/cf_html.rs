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

    // Reproduction tests for the upstream `clipboard_rs` panic observed in
    // production (Sentry issue UNICLIPBOARD-RUST-1V, events
    // `bffcf352449d47c8a903d5cafd16a08e` and `29c606eab66b49fea65ef4471562c431`).
    //
    // `clipboard_rs::platform::win::extract_html_from_clipboard_data` (win.rs:632)
    // takes the `EndHTML:NNNNNNNNNN` byte offset parsed from the CF_HTML
    // header and directly slices the UTF-8 buffer:
    //
    //     Ok(data[start_idx..end_idx].to_string())
    //
    // Some source applications miscompute that offset by 1-2 bytes when the
    // payload contains multi-byte UTF-8 characters (CJK in particular). When
    // the offset lands inside such a character the std slice operation aborts
    // with `byte index N is not a char boundary; it is inside 'X' (bytes A..B)`.
    //
    // These tests pin the exact failure mode so any future defensive shim
    // (catch_unwind wrapper or a char-boundary-aware fallback) has a regression
    // gate to defend against.
    mod cf_html_endhtml_panic_repro {
        /// Minimal reproduction of `clipboard_rs::win::extract_html_from_clipboard_data`'s
        /// fatal line. Kept as a free function so the panic surface is identical
        /// to the upstream call site.
        fn slice_like_clipboard_rs(data: &str, start_idx: usize, end_idx: usize) -> String {
            data[start_idx..end_idx].to_string()
        }

        /// Build a payload whose byte length puts a 3-byte CJK char (`'插'`,
        /// UTF-8 `e6 8f 92`) straddling a target offset, and return that
        /// offset so the caller can use it as a bogus `EndHTML` value.
        fn build_payload_with_endhtml_inside_cjk(prefix_padding: usize) -> (String, usize) {
            let mut buf = String::new();
            buf.push_str("<html>\r\n<body>\r\n<!--StartFragment-->");
            for _ in 0..prefix_padding {
                buf.push('A');
            }
            // `'插'` starts at `buf.len()`; offset +1 lands inside its second byte.
            let end_idx_inside_char = buf.len() + 1;
            buf.push('插');
            buf.push_str("<!--EndFragment-->\r\n</body>\r\n</html>");
            (buf, end_idx_inside_char)
        }

        #[test]
        #[should_panic(expected = "is not a char boundary")]
        fn endhtml_offset_inside_chinese_char_panics() {
            let (data, end_idx) = build_payload_with_endhtml_inside_cjk(100);
            let _ = slice_like_clipboard_rs(&data, 0, end_idx);
        }

        #[test]
        #[should_panic(expected = "inside '插'")]
        fn panic_message_matches_production_signature() {
            // Mirrors the exact wording observed in Sentry so reading the
            // production stacktrace next to this test is unambiguous.
            let (data, end_idx) = build_payload_with_endhtml_inside_cjk(6784);
            let _ = slice_like_clipboard_rs(&data, 0, end_idx);
        }
    }
}
