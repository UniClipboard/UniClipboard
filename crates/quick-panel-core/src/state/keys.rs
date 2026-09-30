//! What each key does, given the state of the panel.
//!
//! The shell hands over the key as it came from the window and the state answers with effects.
//! Whether the key was taken matters: an untaken key goes on to the search box.

use crate::grid::{self, Direction};

use super::{Ctx, Effect, Effects, HostRequest, PanelState};
use crate::ports::EntryAction;

/// The modifier keys held with a key.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Modifiers {
    /// Command on macOS, the Windows key elsewhere.
    pub platform: bool,
    pub control: bool,
    pub shift: bool,
    pub alt: bool,
}

/// The outcome of a key or a keyboard action.
#[derive(Debug, Default)]
pub struct KeyResult {
    /// The panel used the key, so nothing else may.
    pub consumed: bool,
    pub effects: Effects,
}

impl KeyResult {
    fn taken(effects: Effects) -> Self {
        Self {
            consumed: true,
            effects,
        }
    }

    fn ignored() -> Self {
        Self::default()
    }
}

impl PanelState {
    /// A key press in the panel window. `key` is the key name, such as `enter` or `a`.
    pub fn on_key(&mut self, key: &str, modifiers: Modifiers, ctx: &Ctx) -> KeyResult {
        if !self.session.visible || ctx.composing {
            return KeyResult::ignored();
        }
        let command = modifiers.platform || modifiers.control;
        let mut effects = vec![];
        if key == "k" && command && !modifiers.shift {
            if self.menu.is_some() {
                effects.extend(self.close_actions());
            } else {
                effects.extend(self.open_actions(ctx));
            }
            return KeyResult::taken(effects);
        }
        if self.menu.is_some() && self.action_list_key(key, &modifiers, ctx, &mut effects) {
            return KeyResult::taken(effects);
        }
        if key == "escape" {
            if !self.suggestion_options(ctx).is_empty() {
                self.suggest.open = false;
            } else if !ctx.input.is_empty() || !self.search.filters.chips().is_empty() {
                effects.extend(self.clear());
            } else {
                effects.push(Effect::HideWindow);
            }
            return KeyResult::taken(effects);
        }
        if self.search.locked && key == "enter" {
            effects.extend(self.ask_host(HostRequest::ShowMainWindow));
            return KeyResult::taken(effects);
        }
        if command && key == "o" && modifiers.shift {
            effects.extend(self.ask_host(HostRequest::ShowMainWindow));
            return KeyResult::taken(effects);
        }
        if command && key == "," && !modifiers.shift {
            effects.extend(self.ask_host(HostRequest::OpenSettings));
            return KeyResult::taken(effects);
        }
        if key == "l" && command && !modifiers.shift && self.search.disconnected.is_some() {
            effects.push(Effect::OpenLogs);
            return KeyResult::taken(effects);
        }
        if !command && !modifiers.shift && !modifiers.alt && self.menu.is_none() {
            if matches!(key, "up" | "down") && self.arrow_between_zones(key == "down", ctx) {
                return KeyResult::taken(effects);
            }
            if key == "enter" && self.suggest.focused {
                if let Some(accepted) = self.accept_suggestion(ctx) {
                    effects.extend(accepted);
                    return KeyResult::taken(effects);
                }
            }
        }
        if !self.search.loading
            && !self.session.busy
            && self.search.items.is_empty()
            && !command
            && !modifiers.shift
        {
            let shown = self.visible_relaxations().len();
            match key {
                "enter" if self.search.disconnected.is_some() => effects.extend(self.search()),
                "enter" if shown > 0 => effects.extend(self.apply_relaxation()),
                "up" | "down" if shown > 0 => {
                    let cursor = self.search.relax_cursor;
                    self.search.relax_cursor = if key == "down" {
                        (cursor + 1) % shown
                    } else {
                        (cursor + shown - 1) % shown
                    };
                }
                _ => {}
            }
            if matches!(key, "enter" | "up" | "down") {
                return KeyResult::taken(effects);
            }
        }
        if self.search.loading || self.session.busy {
            return if matches!(key, "enter" | "up" | "down") {
                KeyResult::taken(effects)
            } else {
                KeyResult::ignored()
            };
        }
        let grid = self.search.filters.images_only();
        match key {
            "up" | "down" if grid => {
                let direction = if key == "up" {
                    Direction::Up
                } else {
                    Direction::Down
                };
                effects.extend(self.grid_step(direction));
            }
            "up" | "down" => {
                effects.extend(self.step_selection(if key == "up" { -1 } else { 1 }));
            }
            "n" | "p" if modifiers.control => {
                effects.extend(self.step_selection(if key == "p" { -1 } else { 1 }));
            }
            // Enter pastes, Shift+Enter pastes plain text, Command+Enter pastes and keeps the panel.
            "enter" => {
                effects.extend(self.restore(true, modifiers.shift && !command, command, ctx));
            }
            "v" if command && ctx.input.is_empty() => {
                effects.extend(self.restore(true, modifiers.shift, false, ctx));
            }
            "o" if command && !modifiers.shift => effects.extend(self.open_selected()),
            // Deleting an entry is destructive, so it needs Command/Ctrl+Shift+Backspace.
            // Plain Option+Backspace must stay with the search input (delete previous word).
            "backspace" if command && modifiers.shift => {
                if let Some(id) = self.active_item().map(|item| item.entry_id.clone()) {
                    effects.extend(self.entry_action(id, EntryAction::Delete));
                }
            }
            digit if command && digit.len() == 1 && digit.as_bytes()[0].is_ascii_digit() => {
                // Command+1 to Command+9 paste the rows, or the grid cells, that carry those digits.
                let number = key.parse::<usize>().unwrap_or(0);
                let len = self.search.items.len();
                let ix = if grid {
                    grid::entry_for_number(number, self.search.grid_top, len)
                } else {
                    let ix = number.wrapping_sub(1);
                    (ix < VISIBLE_ROWS && ix < len).then_some(ix)
                };
                if let Some(ix) = ix {
                    effects.extend(self.select(ix));
                    effects.extend(self.restore(true, modifiers.shift, false, ctx));
                }
            }
            _ => return KeyResult::ignored(),
        }
        KeyResult::taken(effects)
    }

    /// Backspace in an empty search box removes the last filter chip. The input binds Backspace to
    /// its own deletion, which runs before a plain key listener, so the panel takes the action.
    pub fn on_backspace(&mut self, ctx: &Ctx) -> KeyResult {
        if self.session.visible && self.menu.is_none() && ctx.input.is_empty() {
            // Nothing to delete in the box: remove the last chip if there is one, and in any
            // case keep the input from reporting an edit that would search again.
            let effects = match self.search.filters.chips().last().cloned() {
                Some((dimension, value)) => self.remove_filter(dimension, &value),
                None => vec![],
            };
            return KeyResult::taken(effects);
        }
        KeyResult::ignored()
    }

    /// Command+Backspace (Ctrl+Backspace off macOS): clears the search and its filters.
    pub fn on_clear_all(&mut self) -> KeyResult {
        if !self.session.visible {
            return KeyResult::ignored();
        }
        KeyResult::taken(self.clear())
    }

    /// Left and Right move the grid selection while the search box is empty; with text in it
    /// they stay with the caret.
    pub fn on_sideways(&mut self, direction: Direction, ctx: &Ctx) -> KeyResult {
        let free = self.session.visible
            && self.search.filters.images_only()
            && !self.search.loading
            && self.menu.is_none()
            && ctx.input.is_empty();
        if free {
            KeyResult::taken(self.grid_step(direction))
        } else {
            KeyResult::ignored()
        }
    }

    /// Copy with nothing selected in the search box copies the selected entry instead.
    pub fn on_copy(&mut self, ctx: &Ctx) -> KeyResult {
        if ctx.composing || ctx.selecting {
            return KeyResult::ignored();
        }
        KeyResult::taken(self.restore(false, false, false, ctx))
    }
}

/// Rows that carry a Command+digit shortcut.
pub const VISIBLE_ROWS: usize = 9;
