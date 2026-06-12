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

/// Byte-safe replacement for `clipboard_rs::platform::win::get_html` /
/// `extract_html_from_clipboard_data`.
///
/// The upstream implementation parses the `StartHTML`/`EndHTML` byte offsets
/// from the CF_HTML header and then slices the buffer as a UTF-8 `&str`:
///
/// ```ignore
/// Ok(data[start_idx..end_idx].to_string())
/// ```
///
/// Some source applications (observed: Chinese-language Office, some chat
/// clients) miscompute those offsets by 1-2 bytes when the payload contains
/// multi-byte UTF-8 characters. When the offset lands inside such a character
/// `std`'s `str` indexing aborts the process — see the production panic
/// reproduced in `cf_html_endhtml_panic_repro` below and Sentry issue
/// UNICLIPBOARD-RUST-1V.
///
/// This function reproduces the same intent — return the byte range
/// `[StartHTML, EndHTML)` of the CF_HTML buffer as a `String` — but works
/// on raw bytes throughout:
/// 1. ASCII header lines are parsed byte-by-byte (no UTF-8 dependency).
/// 2. The payload slice is taken from the raw byte buffer.
/// 3. The final `String` is produced via `String::from_utf8_lossy`, so a
///    bad offset that splits a CJK / emoji codepoint degrades to one
///    `U+FFFD` per malformed byte instead of panicking.
///
/// When `StartHTML` / `EndHTML` cannot be parsed (header missing or
/// malformed), the whole buffer is returned lossy — matching upstream's
/// `start_idx = 0; end_idx = data.len()` fallback.
///
/// Returns `None` only for an empty buffer.
#[cfg(any(test, target_os = "windows"))]
pub(crate) fn read_cf_html_payload_from_bytes(bytes: &[u8]) -> Option<String> {
    if bytes.is_empty() {
        return None;
    }

    let (start, end) = parse_cf_html_offsets(bytes).unwrap_or((0, bytes.len()));
    let buf_len = bytes.len();
    let start = start.min(buf_len);
    let end = end.min(buf_len).max(start);
    Some(String::from_utf8_lossy(&bytes[start..end]).into_owned())
}

/// Parse the `StartHTML` and `EndHTML` byte offsets from a CF_HTML buffer.
///
/// The CF_HTML header is required to be pure ASCII (Microsoft spec), which
/// makes a byte-level scan strictly safer than running the buffer through
/// `str::from_utf8` first — a malformed payload section cannot poison header
/// parsing. Header scanning stops at the first line without a `:` separator
/// (i.e. the start of the actual HTML body).
#[cfg(any(test, target_os = "windows"))]
fn parse_cf_html_offsets(bytes: &[u8]) -> Option<(usize, usize)> {
    let mut start_html: Option<usize> = None;
    let mut end_html: Option<usize> = None;

    for line in bytes.split(|&b| b == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let Some(colon) = line.iter().position(|&b| b == b':') else {
            break;
        };
        let key = &line[..colon];
        let value = &line[colon + 1..];
        match key {
            b"StartHTML" => {
                if let Some(v) = parse_zero_padded_ascii_usize(value) {
                    start_html = Some(v);
                }
            }
            b"EndHTML" => {
                if let Some(v) = parse_zero_padded_ascii_usize(value) {
                    end_html = Some(v);
                }
            }
            _ => {}
        }
        if start_html.is_some() && end_html.is_some() {
            break;
        }
    }

    match (start_html, end_html) {
        (Some(s), Some(e)) => Some((s, e)),
        _ => None,
    }
}

/// Byte-level equivalent of `value.trim_start_matches('0').parse::<usize>()`,
/// preserving upstream's quirk that a value of "0000000000" trims to "0" and
/// parses as `0` (not an empty string that fails to parse).
#[cfg(any(test, target_os = "windows"))]
fn parse_zero_padded_ascii_usize(value: &[u8]) -> Option<usize> {
    let mut i = 0;
    while i + 1 < value.len() && value[i] == b'0' {
        i += 1;
    }
    let digits = std::str::from_utf8(&value[i..]).ok()?;
    digits.parse().ok()
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
        use super::super::read_cf_html_payload_from_bytes;

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
        fn endhtml_offset_inside_chinese_char_panics_via_str_slice() {
            // Documents the unfixed upstream behavior: `data[..end_idx]` aborts
            // when `end_idx` lands inside a multi-byte char. This is the bug we
            // are working around.
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

        /// Build a complete CF_HTML buffer (with valid `Version`/`StartHTML`/
        /// `EndHTML` header) where the parsed `EndHTML` byte offset lands
        /// inside a 3-byte CJK character (`'插'`). This is the exact shape of
        /// the buffer that triggers the production panic.
        fn build_cf_html_buffer_with_endhtml_inside_cjk() -> Vec<u8> {
            // Header layout we'll build:
            //   Version:0.9\r\n
            //   StartHTML:0000000105\r\n  (StartHTML is the byte offset of the body)
            //   EndHTML:<10-digit offset>\r\n
            //   StartFragment:<10-digit offset>\r\n
            //   EndFragment:<10-digit offset>\r\n
            // We hand-build the header so its length is exactly 105 bytes — that
            // matches the StartHTML value in the production stacktrace and lets
            // us cleanly compute "where does EndHTML need to point so the
            // offset lands inside '插'".
            let body_prefix = "<html>\r\n<body>\r\n<!--StartFragment-->";
            // Choose a padding so the CJK char starts at a known body offset.
            let padding = "A".repeat(50);
            let body_pre_cjk_len = body_prefix.len() + padding.len();

            // We want the EndHTML offset (header_len + bytes_into_body) to fall
            // inside the second byte of '插'. `'插'` is 3 bytes; the offset
            // `header_len + body_pre_cjk_len + 1` is inside the character.
            let header = "Version:0.9\r\n\
                          StartHTML:0000000105\r\n\
                          EndHTML:0000000000\r\n\
                          StartFragment:0000000000\r\n\
                          EndFragment:0000000000\r\n";
            assert_eq!(
                header.len(),
                105,
                "test fixture broke: header_len shifted, recompute StartHTML"
            );
            let end_html_offset = header.len() + body_pre_cjk_len + 1;

            // Re-emit the header with the real EndHTML value.
            let header = format!(
                "Version:0.9\r\n\
                 StartHTML:0000000105\r\n\
                 EndHTML:{end_html_offset:010}\r\n\
                 StartFragment:0000000000\r\n\
                 EndFragment:0000000000\r\n"
            );
            assert_eq!(header.len(), 105, "header length must stay 105");

            let mut buf = Vec::new();
            buf.extend_from_slice(header.as_bytes());
            buf.extend_from_slice(body_prefix.as_bytes());
            buf.extend_from_slice(padding.as_bytes());
            buf.extend_from_slice("插".as_bytes());
            buf.extend_from_slice("<!--EndFragment-->\r\n</body>\r\n</html>".as_bytes());
            buf
        }

        #[test]
        fn byte_safe_reader_does_not_panic_on_bad_endhtml_offset() {
            // Regression gate for Sentry UNICLIPBOARD-RUST-1V: the byte-safe
            // path must accept the same buffer that aborts the std string
            // slicer and return *some* String (even if the broken codepoint
            // becomes a U+FFFD).
            let buf = build_cf_html_buffer_with_endhtml_inside_cjk();
            let out = read_cf_html_payload_from_bytes(&buf)
                .expect("non-empty CF_HTML buffer must yield Some");

            // Must contain the well-formed prefix that precedes the truncated
            // CJK char, proving we sliced the right range — not just bailed.
            assert!(out.contains("<!--StartFragment-->"));
            assert!(
                out.contains("AAAA"),
                "padding before the bad codepoint should survive: {out:?}"
            );
        }

        #[test]
        fn byte_safe_reader_returns_full_payload_on_well_formed_buffer() {
            // Happy path: when the header offsets are correct, the parser must
            // return the StartHTML..EndHTML slice verbatim (same behavior the
            // production code relied on before this fix).
            //
            // Mirror what `clipboard-win::raw::set_html` would produce.
            let body = "<html>\r\n<body>\r\n<!--StartFragment--><p>hi</p><!--EndFragment-->\r\n</body>\r\n</html>";
            let header = format!(
                "Version:0.9\r\n\
                 StartHTML:0000000105\r\n\
                 EndHTML:{end:010}\r\n\
                 StartFragment:0000000000\r\n\
                 EndFragment:0000000000\r\n",
                end = 105 + body.len(),
            );
            assert_eq!(header.len(), 105);
            let mut buf = Vec::new();
            buf.extend_from_slice(header.as_bytes());
            buf.extend_from_slice(body.as_bytes());

            let out = read_cf_html_payload_from_bytes(&buf).unwrap();
            assert_eq!(out, body);
        }

        #[test]
        fn byte_safe_reader_falls_back_to_full_buffer_when_header_missing() {
            // Defensive: a CF_HTML-like buffer that lacks the StartHTML/EndHTML
            // header must not be silently dropped — it should round-trip as a
            // lossy String spanning the whole buffer.
            let raw = b"<html><body><p>no header</p></body></html>";
            let out = read_cf_html_payload_from_bytes(raw).unwrap();
            assert_eq!(out, std::str::from_utf8(raw).unwrap());
        }

        #[test]
        fn byte_safe_reader_returns_none_for_empty_buffer() {
            assert!(read_cf_html_payload_from_bytes(&[]).is_none());
        }
    }
}
