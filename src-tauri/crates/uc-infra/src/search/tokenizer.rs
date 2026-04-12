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

use std::collections::HashSet;

use unicode_normalization::UnicodeNormalization;
use unicode_segmentation::UnicodeSegmentation;

/// Stateless deterministic tokenizer.
///
/// Call `tokenize_segment()` or `tokenize_all()` to produce normalized tokens.
pub struct SearchTokenizer;

impl SearchTokenizer {
    /// Tokenize a single raw segment and return all normalized tokens.
    ///
    /// Includes prefix expansion (length 3..N-1) for word-level tokens so that
    /// partial queries like `"uniclip"` match `"uniclipboard"`. Use this for
    /// identifier-rich fields: file names, file paths, URLs.
    ///
    /// For large free-text body fields, prefer [`tokenize_segment_no_prefix`] to
    /// avoid the O(unique_tokens × text_length) cost from `count_raw_tokens`.
    pub fn tokenize_segment(&self, raw: &str) -> Vec<String> {
        self.tokenize_segment_inner(raw, true)
    }

    /// Tokenize a single raw segment **without** prefix expansion.
    ///
    /// Identical to [`tokenize_segment`] but skips the prefix-expansion step.
    /// Use this for body / HTML fields where the text can be large and prefix
    /// matching offers little UX value compared to its indexing cost.
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

        // Prefix expansion: for every word-level token (no separators, non-CJK),
        // emit all prefixes of length 3..(token_len - 1) so that partial queries
        // like "uniclip" match entries indexed under "uniclipboard".
        // Skipped for body/html fields (with_prefixes = false) to avoid O(M×N) blowup.
        if with_prefixes {
            let base_tokens: Vec<String> = result.clone();
            for tok in &base_tokens {
                if !contains_separator(tok) && cjk_bigrams(tok).is_empty() {
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

/// Generate all prefixes of length 3..(token_len - 1) for a word-level token.
///
/// Returns an empty vec for tokens with fewer than 4 characters (no useful
/// prefix shorter than the token itself can be generated at min-length 3).
fn prefix_tokens(token: &str) -> Vec<String> {
    let chars: Vec<char> = token.chars().collect();
    if chars.len() <= 3 {
        return vec![];
    }
    // lengths 3, 4, ..., chars.len()-1  (full token already in result)
    (3..chars.len())
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

    #[test]
    fn tokenizer_emits_lowercase_nfkc_tokens() {
        let t = SearchTokenizer;
        // "ＨＥＬＬＯＷｏｒｌｄ" — fullwidth ASCII, NFKC → "HELLOWorld"
        let tokens = t.tokenize_segment("ＨＥＬＬＯＷｏｒｌｄ");
        for tok in &tokens {
            // All tokens must be lowercase ASCII-compatible
            assert_eq!(*tok, tok.to_lowercase(), "token not lowercase: '{tok}'");
        }
        // Should produce tokens containing "hello" and/or "world"
        let all = tokens.join(" ");
        assert!(
            all.contains("hello") || all.contains("world") || all.contains("helloworld"),
            "expected hello or world in: {all:?}"
        );
    }

    #[test]
    fn tokenizer_preserves_whole_identifier_and_split_tokens() {
        let t = SearchTokenizer;
        // "fooBar_baz/qux.txt" should produce:
        // whole: "foobar_baz/qux.txt"
        // splits: "foo", "bar", "baz", "qux", "txt"
        let tokens = t.tokenize_segment("fooBar_baz/qux.txt");

        // Check for presence of split tokens
        assert!(
            tokens.iter().any(|t| t == "foo"),
            "expected 'foo' in {tokens:?}"
        );
        assert!(
            tokens.iter().any(|t| t == "bar"),
            "expected 'bar' in {tokens:?}"
        );
        assert!(
            tokens.iter().any(|t| t == "baz"),
            "expected 'baz' in {tokens:?}"
        );
        assert!(
            tokens.iter().any(|t| t == "qux"),
            "expected 'qux' in {tokens:?}"
        );
        assert!(
            tokens.iter().any(|t| t == "txt"),
            "expected 'txt' in {tokens:?}"
        );

        // Whole identifier should also be present
        assert!(
            tokens.iter().any(|t| t.contains("foobar")),
            "expected normalized whole identifier in {tokens:?}"
        );
    }

    #[test]
    fn tokenizer_drops_single_char_latin_noise() {
        let t = SearchTokenizer;
        let tokens = t.tokenize_segment("a b c hello");
        // Single-char Latin tokens 'a', 'b', 'c' should be dropped
        assert!(
            !tokens.contains(&"a".to_string()),
            "'a' should be dropped: {tokens:?}"
        );
        assert!(
            !tokens.contains(&"b".to_string()),
            "'b' should be dropped: {tokens:?}"
        );
        assert!(
            !tokens.contains(&"c".to_string()),
            "'c' should be dropped: {tokens:?}"
        );
        // "hello" should be kept
        assert!(
            tokens.contains(&"hello".to_string()),
            "expected 'hello' in {tokens:?}"
        );
    }

    #[test]
    fn tokenizer_keeps_cjk_bigrams() {
        let t = SearchTokenizer;
        // "中文测试" → bigrams: "中文", "文测", "测试"
        let tokens = t.tokenize_segment("中文测试");
        assert!(
            tokens.contains(&"中文".to_string()),
            "expected '中文' bigram in {tokens:?}"
        );
        assert!(
            tokens.contains(&"文测".to_string()),
            "expected '文测' bigram in {tokens:?}"
        );
        assert!(
            tokens.contains(&"测试".to_string()),
            "expected '测试' bigram in {tokens:?}"
        );
    }

    #[test]
    fn tokenizer_handles_camelcase_splitting() {
        let t = SearchTokenizer;
        let tokens = t.tokenize_segment("getUserName");
        assert!(
            tokens.iter().any(|t| t == "get"),
            "expected 'get' in {tokens:?}"
        );
        assert!(
            tokens.iter().any(|t| t == "user"),
            "expected 'user' in {tokens:?}"
        );
        assert!(
            tokens.iter().any(|t| t == "name"),
            "expected 'name' in {tokens:?}"
        );
    }

    #[test]
    fn prefix_tokens_generates_length_3_to_n_minus_1() {
        // "hello" (5 chars) → "hel", "hell"
        let prefixes = prefix_tokens("hello");
        assert_eq!(prefixes, vec!["hel", "hell"]);
    }

    #[test]
    fn prefix_tokens_skips_short_tokens() {
        assert!(prefix_tokens("hi").is_empty());
        assert!(prefix_tokens("hey").is_empty());
    }

    #[test]
    fn tokenize_segment_includes_prefix_tokens_for_long_word() {
        let t = SearchTokenizer;
        let tokens = t.tokenize_segment("uniclipboard");
        // Must contain prefix tokens
        assert!(
            tokens.iter().any(|t| t == "uni"),
            "missing 'uni': {tokens:?}"
        );
        assert!(
            tokens.iter().any(|t| t == "unic"),
            "missing 'unic': {tokens:?}"
        );
        assert!(
            tokens.iter().any(|t| t == "uniclip"),
            "missing 'uniclip': {tokens:?}"
        );
        // Full token must still be present
        assert!(
            tokens.iter().any(|t| t == "uniclipboard"),
            "missing 'uniclipboard': {tokens:?}"
        );
    }

    #[test]
    fn tokenize_segment_no_prefix_expansion_of_identifier_token() {
        let t = SearchTokenizer;
        // "foo-bar" → whole identifier "foo-bar" is kept as-is (by design),
        // but the prefix expander must NOT generate "foo-" style partial identifiers.
        let tokens = t.tokenize_segment("foo-bar");
        // The only token allowed to contain '-' is the whole identifier itself.
        let separator_tokens: Vec<&String> = tokens.iter().filter(|t| t.contains('-')).collect();
        assert_eq!(
            separator_tokens,
            vec!["foo-bar"],
            "only the whole identifier may contain a separator: {tokens:?}"
        );
    }

    #[test]
    fn tokenize_all_deduplicates_across_segments() {
        let t = SearchTokenizer;
        let segs = vec!["hello world".to_string(), "hello rust".to_string()];
        let tokens = t.tokenize_all(&segs);
        // "hello" should appear exactly once
        let hello_count = tokens.iter().filter(|t| t.as_str() == "hello").count();
        assert_eq!(hello_count, 1, "hello should be deduplicated: {tokens:?}");
    }

    #[test]
    fn cjk_bigrams_from_pure_cjk_string() {
        let bigrams = cjk_bigrams("中文");
        assert_eq!(bigrams, vec!["中文"]);
    }

    #[test]
    fn cjk_bigrams_three_chars() {
        let bigrams = cjk_bigrams("中文测");
        assert_eq!(bigrams, vec!["中文", "文测"]);
    }

    #[test]
    fn should_keep_single_latin_false() {
        assert!(!should_keep_token("a"));
        assert!(!should_keep_token("z"));
    }

    #[test]
    fn should_keep_multi_char_token() {
        assert!(should_keep_token("hello"));
        assert!(should_keep_token("中文"));
    }
}
