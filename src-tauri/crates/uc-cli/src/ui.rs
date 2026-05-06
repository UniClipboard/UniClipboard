//! CLI 输出样式和交互输入辅助函数。

use console::{style, Key, Style, Term};
use dialoguer::{theme::Theme, Confirm, Input};
use indicatif::{ProgressBar, ProgressStyle};
use std::fmt;

// ── Colour palette ──────────────────────────────────────────────────

fn cyan() -> Style {
    Style::new().cyan()
}

fn green() -> Style {
    Style::new().green()
}

fn yellow() -> Style {
    Style::new().yellow()
}

fn red() -> Style {
    Style::new().red()
}

fn dim() -> Style {
    Style::new().dim()
}

fn bold() -> Style {
    Style::new().bold()
}

// ── Structured messages ─────────────────────────────────────────────

/// Print a section header: `◆  Title`
pub fn header(text: &str) {
    let term = Term::stderr();
    let _ = term.write_line(&format!(
        "\n {}  {}",
        style("◆").cyan().bold(),
        bold().apply_to(text)
    ));
}

/// Print a success line: `✓  Message`
pub fn success(text: &str) {
    let term = Term::stderr();
    let _ = term.write_line(&format!(" {}  {}", green().apply_to("✓"), text));
}

/// Print a warning line: `⚠  Message`
pub fn warn(text: &str) {
    let term = Term::stderr();
    let _ = term.write_line(&format!(" {}  {}", yellow().apply_to("⚠"), text));
}

/// Print an error line: `✗  Message`
pub fn error(text: &str) {
    let term = Term::stderr();
    let _ = term.write_line(&format!(" {}  {}", red().apply_to("✗"), text));
}

/// Print an info/detail line with dim prefix.
pub fn info(label: &str, value: &str) {
    let term = Term::stderr();
    let _ = term.write_line(&format!(
        " {}  {} {}",
        dim().apply_to("│"),
        dim().apply_to(format!("{label}:")),
        value,
    ));
}

/// Print a dim separator bar.
pub fn bar() {
    let term = Term::stderr();
    let _ = term.write_line(&format!(" {}", dim().apply_to("│")));
}

/// Print a closing corner: `└  Message`
pub fn end(text: &str) {
    let term = Term::stderr();
    let _ = term.write_line(&format!(
        " {}  {}",
        green().apply_to("└"),
        green().apply_to(text),
    ));
}

// ── Interactive prompts ─────────────────────────────────────────────

/// dialoguer theme that matches `ui.rs` glyph + spacing conventions
/// (leading space + glyph + double space + content). Active prompts use
/// a yellow `?`; resolved prompts collapse to the canonical green `✓`
/// line so the transcript stays consistent with `success` / `error`.
struct UniclipTheme;

impl Theme for UniclipTheme {
    fn format_confirm_prompt(
        &self,
        f: &mut dyn fmt::Write,
        prompt: &str,
        default: Option<bool>,
    ) -> fmt::Result {
        let q = yellow().apply_to("?");
        let suffix = match default {
            None => "[y/n]",
            Some(true) => "[Y/n]",
            Some(false) => "[y/N]",
        };
        write!(f, " {q}  {prompt} {}", dim().apply_to(suffix))
    }

    fn format_confirm_prompt_selection(
        &self,
        f: &mut dyn fmt::Write,
        prompt: &str,
        selection: Option<bool>,
    ) -> fmt::Result {
        let glyph = green().apply_to("✓");
        let value = match selection {
            Some(true) => "yes",
            Some(false) => "no",
            None => "—",
        };
        write!(f, " {glyph}  {prompt} {}", dim().apply_to(value))
    }

    fn format_input_prompt(
        &self,
        f: &mut dyn fmt::Write,
        prompt: &str,
        default: Option<&str>,
    ) -> fmt::Result {
        let q = yellow().apply_to("?");
        match default {
            Some(d) => write!(f, " {q}  {prompt} [{}] ", dim().apply_to(d)),
            None => write!(f, " {q}  {prompt} "),
        }
    }

    fn format_input_prompt_selection(
        &self,
        f: &mut dyn fmt::Write,
        prompt: &str,
        sel: &str,
    ) -> fmt::Result {
        let glyph = green().apply_to("✓");
        write!(f, " {glyph}  {prompt} {}", dim().apply_to(sel))
    }

    fn format_error(&self, f: &mut dyn fmt::Write, err: &str) -> fmt::Result {
        let glyph = red().apply_to("✗");
        write!(f, " {glyph}  {err}")
    }
}

/// Yes/no confirmation. Renders as ` ?  Prompt [y/N]` while live and
/// collapses to ` ✓  Prompt yes` once the user answers. `default` is
/// returned on bare `Enter`; pass `false` for "must explicitly opt in"
/// flows (e.g. the LAN exposure warning).
pub fn confirm(prompt: &str, default: bool) -> Result<bool, String> {
    Confirm::with_theme(&UniclipTheme)
        .with_prompt(prompt)
        .default(default)
        .interact_on(&Term::stderr())
        .map_err(|e| e.to_string())
}

/// Single-line text prompt. `allow_empty=true` accepts an empty
/// submission (used for `[Enter for auto]` flows); `false` re-prompts
/// until the user types something non-empty.
pub fn input(prompt: &str, allow_empty: bool) -> Result<String, String> {
    Input::<String>::with_theme(&UniclipTheme)
        .with_prompt(prompt)
        .allow_empty(allow_empty)
        .interact_text_on(&Term::stderr())
        .map_err(|e| e.to_string())
}

/// Show a masked password prompt (displays `•` per character).
pub fn password(prompt: &str) -> Result<String, String> {
    read_masked_password(prompt)
}

/// Show a masked password prompt with confirmation.
pub fn password_with_confirm(prompt: &str, confirm_prompt: &str) -> Result<String, String> {
    loop {
        let p1 = read_masked_password(prompt)?;
        let p2 = read_masked_password(confirm_prompt)?;
        if p1 == p2 {
            return Ok(p1);
        }
        // Route through the canonical `error` helper so alignment/colour
        // match every other error line in the CLI.
        error("Passphrases do not match, try again");
    }
}

const MASK_CHAR: &str = "•";

/// Read a password with masked feedback on stderr.
///
/// Renders as two lines during input (note: every glyph is followed by
/// **two** spaces so the input line and the prompt label align in the
/// same content column, matching `confirm` / `input` / `success` /
/// `error` everywhere else in the CLI):
/// ```text
///  ?  Prompt label
///  │  ••••
/// ```
/// Collapses to one line after Enter, mirroring `format_input_prompt_selection`:
/// ```text
///  ✓  Prompt label ••••••••
/// ```
fn read_masked_password(prompt: &str) -> Result<String, String> {
    let term = Term::stderr();

    // Line 1: prompt label — yellow `?` + double space matches the live
    // dialoguer prompt rendering used by `ui::confirm` / `ui::input`.
    let _ = term.write_line(&format!(" {}  {}", yellow().apply_to("?"), prompt));
    // Line 2: input line with bar prefix + cursor (double space already).
    let bar_prefix = format!(" {}  ", dim().apply_to("│"));
    let _ = term.write_str(&bar_prefix);
    let _ = term.flush();

    let mut input = String::new();
    loop {
        let key = term
            .read_key()
            .map_err(|e| format!("password input failed: {e}"))?;
        match key {
            Key::Enter => {
                // Clear the two live lines (input line + prompt line)
                let _ = term.clear_line();
                let _ = term.move_cursor_up(1);
                let _ = term.clear_line();
                // Rewrite as single collapsed line — green `✓` + double
                // space mirrors `format_input_prompt_selection` so the
                // resolved transcript reads consistently.
                let mask: String = MASK_CHAR.repeat(input.len());
                let _ = term.write_line(&format!(
                    " {}  {} {}",
                    green().apply_to("✓"),
                    prompt,
                    dim().apply_to(mask),
                ));
                return Ok(input);
            }
            Key::Backspace => {
                if !input.is_empty() {
                    input.pop();
                    let _ = term.write_str("\x08 \x08");
                    let _ = term.flush();
                }
            }
            Key::Char(c) => {
                input.push(c);
                let _ = term.write_str(MASK_CHAR);
                let _ = term.flush();
            }
            Key::Escape => {
                // Clear live lines
                let _ = term.clear_line();
                let _ = term.move_cursor_up(1);
                let _ = term.clear_line();
                return Err("password input cancelled".to_string());
            }
            _ => {}
        }
    }
}

// ── Spinner ─────────────────────────────────────────────────────────

/// Create and start a spinner with the given message.
///
/// The template uses 2 spaces between glyph and message so the text column
/// matches [`success`] / [`error`] — this way nothing visually shifts when the
/// spinner resolves via [`spinner_finish_success`] / [`spinner_finish_error`].
pub fn spinner(message: &str) -> ProgressBar {
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .tick_strings(&["◒", "◐", "◓", "◑"])
            .template(" {spinner}  {msg}")
            .expect("valid spinner template"),
    );
    pb.set_message(message.to_string());
    pb.enable_steady_tick(std::time::Duration::from_millis(120));
    pb
}

/// Finish spinner with a success message.
///
/// Clears the spinner line entirely, then emits a normal [`success`] line — so
/// alignment, colour, and spacing are controlled by a single source of truth
/// (no duplicated spinner template).
pub fn spinner_finish_success(pb: &ProgressBar, message: &str) {
    pb.finish_and_clear();
    success(message);
}

/// Finish spinner with an error message. See [`spinner_finish_success`] for
/// alignment rationale.
pub fn spinner_finish_error(pb: &ProgressBar, message: &str) {
    pb.finish_and_clear();
    error(message);
}

// ── Verification code display ───────────────────────────────────────

/// Display a verification code prominently.
pub fn verification_code(code: &str) {
    let term = Term::stderr();
    let _ = term.write_line(&format!(
        " {}  Verification code: {}",
        dim().apply_to("│"),
        cyan().bold().apply_to(code),
    ));
}
