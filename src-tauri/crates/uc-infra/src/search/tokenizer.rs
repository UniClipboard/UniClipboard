//! Deterministic text tokenizer for search indexing.
//!
//! Normalization and tokenization rules (applied in order):
//! 1. NFKC Unicode normalization
//! 2. Lowercase
//! 3. Split on Unicode word boundaries (via `unicode-segmentation`)
//! 4. Further split on `_`, `-`, `.`, `/`
//! 5. Split camelCase and PascalCase boundaries
//! 6. For identifier/path-like inputs, preserve the original normalized whole segment plus parts
//! 7. Drop single-character Latin tokens (keep CJK characters)
//! 8. Generate overlapping bigrams over contiguous CJK runs
//!
//! Prefix expansion is decided **per token** (not per field): any non-CJK token
//! whose length lies in `[PREFIX_MIN_LEN, PREFIX_MAX_LEN]` emits all prefixes of
//! length `≥ PREFIX_MIN_LEN`. This lets identifiers embedded in plain text
//! (`localhost`, `apiUserManager`, `192.168.1.1:8080`) match partial queries
//! while capping the index blow-up from very long opaque strings (base64 / JWT /
//! long hashes) at full-token-only.

use std::collections::HashSet;

use unicode_normalization::UnicodeNormalization;
use unicode_segmentation::UnicodeSegmentation;

/// Minimum length (in chars) of a token that participates in prefix expansion.
///
/// Tokens shorter than this are indexed as-is — there is no useful prefix
/// shorter than the token itself.
const PREFIX_MIN_LEN: usize = 3;

/// Maximum length (in chars) of a token that participates in prefix expansion.
///
/// Tokens longer than this are indexed as the full token only. This bounds the
/// index blow-up from long opaque strings (base64 payloads, JWTs, long hashes,
/// raw URLs) where partial-substring search is rarely useful and would otherwise
/// add `O(len)` prefix tokens per occurrence. See `search-internals.mdx`.
const PREFIX_MAX_LEN: usize = 32;

/// Stateless deterministic tokenizer.
///
/// Call `tokenize_segment()` or `tokenize_all()` to produce normalized tokens.
pub struct SearchTokenizer;

impl SearchTokenizer {
    /// Tokenize a single raw segment for **indexing**.
    ///
    /// Emits the base token set plus prefix expansions for every token whose
    /// length lies in `[PREFIX_MIN_LEN, PREFIX_MAX_LEN]` and that is not CJK.
    /// The prefix decision is per-token, not per-field: any plain-text identifier
    /// (`localhost`, `apiUserManager`, host:port) gets prefix expansion regardless
    /// of which field it came from. Use [`tokenize_segment_no_prefix`] at query
    /// time so the user's partial term is looked up as an exact tag.
    pub fn tokenize_segment(&self, raw: &str) -> Vec<String> {
        self.tokenize_segment_inner(raw, true)
    }

    /// Tokenize a single raw segment **without** prefix expansion.
    ///
    /// Used at query time: the user's partial term (e.g. `"loca"`) is looked up
    /// as an exact tag, matching the prefix tags stored at index time.
    pub fn tokenize_segment_no_prefix(&self, raw: &str) -> Vec<String> {
        self.tokenize_segment_inner(raw, false)
    }

    fn tokenize_segment_inner(&self, raw: &str, with_prefixes: bool) -> Vec<String> {
        if raw.is_empty() {
            return vec![];
        }

        // Step 1 & 2: NFKC normalize and lowercase the ORIGINAL for whole-segment preservation.
        let nfkc_original: String = raw.nfkc().collect();
        let lowered = nfkc_original.to_lowercase();

        let is_identifier_like = contains_separator(&lowered) || has_camel_case(&nfkc_original);

        let mut candidate_tokens: Vec<String> = Vec::new();

        // Preserve the normalized whole segment for identifier/path-like inputs.
        if is_identifier_like {
            candidate_tokens.push(lowered.clone());
        }

        // Step 3: Split on Unicode word boundaries (already on lowercased form).
        // This handles natural language splitting. Note: unicode_words() skips separator chars.
        for word in lowered.unicode_words() {
            if !word.is_empty() {
                candidate_tokens.push(word.to_string());
            }
        }

        // Step 4 & 5: Separator + camelCase splitting on the original NFKC form.
        // Apply camelCase split first, then separator split on each part.
        let camel_parts = split_camel_case_original(&nfkc_original);
        for camel_part in &camel_parts {
            // Apply separator splitting to each camelCase part
            for sep_part in split_on_separators(camel_part) {
                let lowered_part = sep_part.to_lowercase();
                if !lowered_part.is_empty() {
                    candidate_tokens.push(lowered_part);
                }
            }
        }

        // Also apply separator splitting directly to the lowered form to catch any missed tokens.
        for sep_part in split_on_separators(&lowered) {
            if !sep_part.is_empty() {
                candidate_tokens.push(sep_part.to_string());
            }
        }

        // Step 7: Filter — drop single-character Latin tokens.
        let filtered: Vec<String> = candidate_tokens
            .into_iter()
            .filter(|t| should_keep_token(t))
            .collect();

        // Step 8: Dedup and CJK bigrams.
        let mut result: Vec<String> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();

        for tok in &filtered {
            if seen.insert(tok.clone()) {
                let bigrams = cjk_bigrams(tok);
                if bigrams.is_empty() {
                    result.push(tok.clone());
                } else {
                    // CJK token: expand to bigrams
                    for bg in bigrams {
                        if seen.insert(bg.clone()) {
                            result.push(bg);
                        }
                    }
                }
            }
        }

        // Add CJK bigrams from the full lowered string (catches multi-char CJK runs).
        for bg in cjk_bigrams(&lowered) {
            if seen.insert(bg.clone()) {
                result.push(bg);
            }
        }

        // Prefix expansion: per-token decision — any non-CJK token whose length
        // is in [PREFIX_MIN_LEN, PREFIX_MAX_LEN] emits all prefixes of length
        // ≥ PREFIX_MIN_LEN. The decision is no longer field-driven: identifiers
        // embedded in plain-text body (e.g. `localhost`, `apiUserManager`,
        // `192.168.1.1:8080`) get the same expansion as URL/file fields.
        //
        // CJK tokens are skipped — they go through the bigram path instead.
        // `with_prefixes` is set to false at query time so the user's partial
        // term is looked up as an exact tag.
        if with_prefixes {
            let base_tokens: Vec<String> = result.clone();
            for tok in &base_tokens {
                if cjk_bigrams(tok).is_empty() {
                    for prefix in prefix_tokens(tok) {
                        if seen.insert(prefix.clone()) {
                            result.push(prefix);
                        }
                    }
                }
            }
        }

        result
    }

    /// Tokenize multiple raw segments and return a deduplicated flat list.
    ///
    /// Includes prefix expansion. Use for identifier-rich index fields (file names,
    /// paths, URLs). For query-time tokenization use [`tokenize_all_no_prefix`].
    pub fn tokenize_all(&self, raw_segments: &[String]) -> Vec<String> {
        self.tokenize_all_inner(raw_segments, true)
    }

    /// Tokenize multiple raw segments without prefix expansion.
    ///
    /// Use at query time: the user's partial term (e.g. `"uniclip"`) is looked up
    /// as an exact token, matching the prefix tokens stored at index time.
    pub fn tokenize_all_no_prefix(&self, raw_segments: &[String]) -> Vec<String> {
        self.tokenize_all_inner(raw_segments, false)
    }

    fn tokenize_all_inner(&self, raw_segments: &[String], with_prefixes: bool) -> Vec<String> {
        let mut seen: HashSet<String> = HashSet::new();
        let mut result = Vec::new();

        for seg in raw_segments {
            for tok in self.tokenize_segment_inner(seg, with_prefixes) {
                if seen.insert(tok.clone()) {
                    result.push(tok);
                }
            }
        }

        result
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Private helpers
// ──────────────────────────────────────────────────────────────────────────────

/// Check whether a character is a separator for identifier splitting.
fn is_separator(ch: char) -> bool {
    matches!(ch, '_' | '-' | '.' | '/')
}

/// Check if the string contains any separator characters.
fn contains_separator(s: &str) -> bool {
    s.chars().any(is_separator)
}

/// Split a string on separator characters `_`, `-`, `.`, `/`.
fn split_on_separators(s: &str) -> Vec<&str> {
    s.split(|c: char| is_separator(c))
        .filter(|p| !p.is_empty())
        .collect()
}

/// Check whether the original string has camelCase transitions
/// (lowercase letter followed by uppercase letter).
fn has_camel_case(s: &str) -> bool {
    let chars: Vec<char> = s.chars().collect();
    for i in 1..chars.len() {
        if chars[i - 1].is_lowercase() && chars[i].is_uppercase() {
            return true;
        }
    }
    false
}

/// Split camelCase/PascalCase on the original (NFKC but not lowercased) string.
///
/// Handles:
/// - "fooBar"     → ["foo", "Bar"]
/// - "getUserName"→ ["get", "User", "Name"]
/// - "HTMLParser" → ["HTML", "Parser"]
///
/// Each returned part retains its original casing; callers must lowercase.
fn split_camel_case_original(s: &str) -> Vec<String> {
    let chars: Vec<char> = s.chars().collect();
    let mut result: Vec<String> = Vec::new();
    let mut current = String::new();

    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];

        if ch.is_uppercase() && !current.is_empty() {
            let prev_lower = current.chars().last().map_or(false, |c| c.is_lowercase());
            let next_lower = chars.get(i + 1).map_or(false, |c| c.is_lowercase());
            let prev_upper = current.chars().last().map_or(false, |c| c.is_uppercase());

            // Transition: lowercase→UPPER or upper-run before UPPER+lower
            if prev_lower || (prev_upper && next_lower && current.len() > 1) {
                // When prev is all upper and we're about to start a new uppercase+lower word
                // (e.g., "HTMLParser": flush "HTM" keep "L", then "Parser" starts)
                if prev_upper && next_lower && current.len() > 1 {
                    // Keep the last char of current to start the new word
                    let last_char = current.pop().unwrap();
                    result.push(current.clone());
                    current.clear();
                    current.push(last_char);
                } else {
                    result.push(current.clone());
                    current.clear();
                }
            }
        }

        current.push(ch);
        i += 1;
    }

    if !current.is_empty() {
        result.push(current);
    }

    result
}

/// Generate all prefixes of length [`PREFIX_MIN_LEN`]..(token_len - 1) for a token.
///
/// Returns an empty vec when:
/// - the token is shorter than or equal to [`PREFIX_MIN_LEN`] (no useful
///   prefix shorter than the token itself), or
/// - the token is longer than [`PREFIX_MAX_LEN`] — long opaque strings
///   (base64 / JWT / hashes / raw URLs) would otherwise add `O(len)` prefix
///   tokens per occurrence with little UX benefit.
fn prefix_tokens(token: &str) -> Vec<String> {
    let chars: Vec<char> = token.chars().collect();
    if chars.len() <= PREFIX_MIN_LEN || chars.len() > PREFIX_MAX_LEN {
        return vec![];
    }
    // lengths PREFIX_MIN_LEN, PREFIX_MIN_LEN+1, ..., chars.len() - 1
    // (full token already in `result`)
    (PREFIX_MIN_LEN..chars.len())
        .map(|end| chars[..end].iter().collect())
        .collect()
}

/// Return true if the token should be kept in the output.
///
/// - Drop single-character tokens that are ASCII letters (Latin).
/// - Keep single CJK characters (they contribute to bigrams).
/// - Keep multi-character tokens.
fn should_keep_token(token: &str) -> bool {
    let chars: Vec<char> = token.chars().collect();
    if chars.len() == 1 {
        let ch = chars[0];
        // Drop single ASCII alphabetic characters
        if ch.is_ascii_alphabetic() {
            return false;
        }
        // Keep CJK — they form bigrams
        if is_cjk(ch) {
            return true;
        }
        // Keep ASCII digits
        if ch.is_ascii_digit() {
            return true;
        }
        // Drop everything else that's a single char
        return false;
    }
    true
}

/// Check whether a character is in a CJK Unicode range.
fn is_cjk(ch: char) -> bool {
    let c = ch as u32;
    // CJK Unified Ideographs
    (0x4E00..=0x9FFF).contains(&c)
        // CJK Extension A
        || (0x3400..=0x4DBF).contains(&c)
        // CJK Compatibility Ideographs
        || (0xF900..=0xFAFF).contains(&c)
        // CJK Unified Ideographs Extension B
        || (0x20000..=0x2A6DF).contains(&c)
        // Hiragana
        || (0x3040..=0x309F).contains(&c)
        // Katakana
        || (0x30A0..=0x30FF).contains(&c)
        // Hangul Syllables
        || (0xAC00..=0xD7AF).contains(&c)
}

/// Generate overlapping bigrams from contiguous CJK runs in the string.
///
/// For `"中文测试"` produces `["中文", "文测", "测试"]`.
fn cjk_bigrams(s: &str) -> Vec<String> {
    let mut bigrams = Vec::new();
    let chars: Vec<char> = s.chars().collect();
    let mut run_start: Option<usize> = None;

    for (i, &ch) in chars.iter().enumerate() {
        if is_cjk(ch) {
            if run_start.is_none() {
                run_start = Some(i);
            }
        } else if let Some(start) = run_start.take() {
            let run = &chars[start..i];
            for window in run.windows(2) {
                bigrams.push(window.iter().collect());
            }
        }
    }

    // Handle run that extends to end of string
    if let Some(start) = run_start {
        let run = &chars[start..];
        for window in run.windows(2) {
            bigrams.push(window.iter().collect());
        }
    }

    bigrams
}

// ──────────────────────────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Returns the index-time tag set for `raw`.
    fn index_tags(raw: &str) -> HashSet<String> {
        SearchTokenizer.tokenize_segment(raw).into_iter().collect()
    }

    /// Returns the query-time tag set for `raw` (no prefix expansion).
    fn query_tags(raw: &str) -> Vec<String> {
        SearchTokenizer.tokenize_segment_no_prefix(raw)
    }

    /// True when every query tag is present in the indexed tag set (AND semantics).
    fn matches(indexed: &HashSet<String>, query: &str) -> bool {
        let q = query_tags(query);
        !q.is_empty() && q.iter().all(|t| indexed.contains(t))
    }

    // ─── #580: identifier-like substrings in plain text ──────────────────────

    #[test]
    fn host_port_in_plain_text_matches_prefix_query() {
        let indexed = index_tags("localhost:3000");
        assert!(
            matches(&indexed, "loca"),
            "indexed tags = {indexed:?} should let `loca` match `localhost:3000`"
        );
        assert!(matches(&indexed, "localhost"));
        assert!(matches(&indexed, "3000"));
    }

    #[test]
    fn ipv4_port_in_plain_text_matches_dotted_query() {
        // Acceptance from #580: copying `192.168.1.1:8080`, search `192.168` hits.
        let indexed = index_tags("192.168.1.1:8080");
        assert!(matches(&indexed, "192.168"));
        assert!(matches(&indexed, "192"));
        assert!(matches(&indexed, "8080"));
    }

    #[test]
    fn camel_case_identifier_matches_prefix_query() {
        // Acceptance from #580: copying `apiUserManager`, search `api` hits.
        let indexed = index_tags("apiUserManager");
        assert!(matches(&indexed, "api"));
        assert!(matches(&indexed, "user"));
        assert!(matches(&indexed, "apiuser"));
        assert!(matches(&indexed, "apiusermanager"));
    }

    #[test]
    fn body_text_now_supports_prefix_search() {
        // Plain English prose embedded in body — same per-token prefix decision
        // applies regardless of which field this came from.
        let indexed = index_tags("uniclipboard release notes");
        assert!(matches(&indexed, "uniclip"));
        assert!(matches(&indexed, "release"));
    }

    // ─── Long-token boundary (>32 chars, no separators) ─────────────────────

    #[test]
    fn long_opaque_token_only_matches_in_full() {
        // 80-char base64-ish blob — must match in full but not by fragment.
        let blob =
            "abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789abcd0001";
        assert_eq!(blob.len(), 80);
        let indexed = index_tags(blob);
        // Indexed as the full lowered token only — no prefix expansion.
        assert!(indexed.contains(&blob.to_lowercase()));
        assert!(!matches(&indexed, "abcd"));
        assert!(matches(&indexed, blob));
    }

    #[test]
    fn token_at_max_len_still_expands() {
        // Boundary: exactly PREFIX_MAX_LEN chars → still expands.
        let token: String = "a".repeat(PREFIX_MAX_LEN);
        let indexed = index_tags(&token);
        // A 3-char prefix of all-`a`s should still be searchable.
        assert!(matches(&indexed, "aaa"));
    }

    #[test]
    fn token_just_over_max_len_does_not_expand() {
        let token: String = "a".repeat(PREFIX_MAX_LEN + 1);
        let indexed = index_tags(&token);
        // Full token still indexed.
        assert!(indexed.contains(&token));
        // No prefix tags emitted.
        let prefix3: String = "a".repeat(3);
        let prefix_below_full: String = "a".repeat(PREFIX_MAX_LEN);
        assert!(!indexed.contains(&prefix3));
        assert!(!indexed.contains(&prefix_below_full));
    }

    // ─── CJK still uses bigrams, not prefix expansion ───────────────────────

    #[test]
    fn cjk_uses_bigrams_not_prefix_expansion() {
        let indexed = index_tags("中文测试");
        // Bigrams present.
        assert!(indexed.contains("中文"));
        assert!(indexed.contains("文测"));
        assert!(indexed.contains("测试"));
        // No 3-char prefix from a CJK token.
        for t in &indexed {
            assert!(t.chars().count() <= 4, "unexpected long CJK token: {t}");
        }
    }

    // ─── Stop / very short tokens are not blown up by expansion ─────────────

    #[test]
    fn three_char_token_is_indexed_as_self_only() {
        let indexed = index_tags("the");
        assert!(indexed.contains("the"));
        // No 2-char prefix and no 1-char prefix were emitted.
        assert!(!indexed.contains("th"));
        assert!(!indexed.contains("t"));
    }
}
