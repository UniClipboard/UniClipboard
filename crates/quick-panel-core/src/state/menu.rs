//! The action list opened with Command+K, shown in the satellite window.

use crate::actions::{self, Action, Row};
use crate::text;

use super::{Ctx, Effect, Effects, HostRequest, PanelState};
use crate::ports::EntryAction;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ActionsPage {
    Main,
    Devices,
}

/// The open action list. It acts on the selected entry.
#[derive(Debug)]
pub struct ActionMenu {
    pub page: ActionsPage,
    pub cursor: usize,
    /// The preview was opened by the list and must be reloaded when the list closes.
    pub forced_preview: bool,
}

/// The action list as the satellite window draws it.
#[derive(Clone, PartialEq, Debug)]
pub struct ActionsView {
    pub title: String,
    pub rows: Vec<Row>,
    pub cursor: usize,
}

/// The rows the list shows now: the entry's actions, or the devices to send to. Actions the
/// platform cannot do are left out.
pub(super) fn rows_of(state: &PanelState, ctx: &Ctx) -> Option<(String, Vec<Row>)> {
    let menu = state.menu.as_ref()?;
    let item = state.active_item()?;
    Some(match menu.page {
        ActionsPage::Main => {
            let rows = actions::rows(item, ctx.target.name().as_deref())
                .into_iter()
                .filter(|row| match row.action {
                    Action::Paste
                    | Action::PastePlain
                    | Action::PasteKeepOpen
                    | Action::PastePaths => ctx.capabilities.auto_paste,
                    Action::Open | Action::RevealFile => ctx.capabilities.open_and_reveal,
                    _ => true,
                })
                .collect();
            (text::ACTIONS.to_string(), rows)
        }
        ActionsPage::Devices => (
            text::SEND_TO.to_string(),
            actions::device_rows(&state.catalog.members),
        ),
    })
}

impl PanelState {
    pub fn actions_view(&self, ctx: &Ctx) -> Option<ActionsView> {
        let (title, rows) = self.action_rows(ctx)?;
        let cursor = self.menu.as_ref()?.cursor.min(rows.len().saturating_sub(1));
        Some(ActionsView {
            title,
            rows,
            cursor,
        })
    }

    fn reset_menu_cursor(&mut self, ctx: &Ctx) {
        if let Some((_, rows)) = self.action_rows(ctx) {
            if let Some(menu) = self.menu.as_mut() {
                menu.cursor = actions::first_enabled(&rows);
            }
        }
    }

    /// Command+K: shows what can be done with the selected entry in the satellite window.
    pub fn open_actions(&mut self, ctx: &Ctx) -> Effects {
        if self.search.loading || self.session.busy || self.menu.is_some() {
            return vec![];
        }
        let Some(item) = self.active_item() else {
            return vec![];
        };
        let id = item.entry_id.clone();
        let forced_preview = self.preview.entry.as_deref() != Some(&id);
        let mut effects = vec![];
        if forced_preview {
            effects.push(Effect::CancelPreview);
            self.preview.entry = Some(id);
            self.preview.text = None;
            self.preview.loading = false;
        }
        self.preview.expanded = true;
        self.menu = Some(ActionMenu {
            page: ActionsPage::Main,
            cursor: 0,
            forced_preview,
        });
        self.reset_menu_cursor(ctx);
        effects.push(Effect::ShowPreviewWindow);
        effects
    }

    pub fn close_actions(&mut self) -> Effects {
        let Some(menu) = self.menu.take() else {
            return vec![];
        };
        if menu.forced_preview {
            // The preview text was never loaded for this entry; load it as if it had just been
            // selected.
            self.preview.entry = None;
            return self.schedule_preview();
        }
        vec![]
    }

    pub(super) fn move_action_cursor(&mut self, forward: bool, ctx: &Ctx) {
        let Some((_, rows)) = self.action_rows(ctx) else {
            return;
        };
        if let Some(menu) = self.menu.as_mut() {
            menu.cursor = actions::step(&rows, menu.cursor.min(rows.len() - 1), forward);
        }
    }

    /// Runs an action from the list, from a click or from Enter.
    pub fn run_action(&mut self, action: Action, ctx: &Ctx) -> Effects {
        let Some(item) = self.active_item().cloned() else {
            return vec![];
        };
        if let Action::ChooseDevice = action {
            self.menu = self.menu.take().map(|menu| ActionMenu {
                page: ActionsPage::Devices,
                cursor: 0,
                ..menu
            });
            self.reset_menu_cursor(ctx);
            return vec![];
        }
        // The list is closed first: pasting hides the panel, and the other actions leave it.
        let mut effects = self.close_actions();
        let id = item.entry_id.clone();
        match action {
            Action::Paste => effects.extend(self.restore(true, false, false, ctx)),
            Action::PastePlain => effects.extend(self.restore(true, true, false, ctx)),
            Action::PasteKeepOpen => effects.extend(self.restore(true, false, true, ctx)),
            Action::Copy => effects.extend(self.restore(false, false, false, ctx)),
            Action::PastePaths => effects.extend(self.paste_paths(item.file_paths, ctx)),
            Action::Open => effects.extend(self.open_selected()),
            Action::RevealFile => {
                if let Some(path) = item.file_paths.iter().find(|p| !p.is_empty()) {
                    effects.push(Effect::RevealPath(path.clone()));
                }
            }
            Action::OpenMainWindow => {
                effects.push(Effect::HostRequest(HostRequest::ShowMainWindow));
            }
            Action::OpenSettings => effects.push(Effect::HostRequest(HostRequest::OpenSettings)),
            Action::Send(peer) => effects.extend(self.entry_action(id, EntryAction::Send(peer))),
            Action::Favorite(value) => {
                effects.extend(self.entry_action(id, EntryAction::Favorite(value)));
            }
            Action::Delete => effects.extend(self.entry_action(id, EntryAction::Delete)),
            Action::ChooseDevice => {}
        }
        effects
    }

    /// Keys while the action list is open. Returns whether the list took the key.
    pub(super) fn action_list_key(
        &mut self,
        key: &str,
        modifiers: &super::keys::Modifiers,
        ctx: &Ctx,
        effects: &mut Effects,
    ) -> bool {
        let Some(page) = self.menu.as_ref().map(|menu| menu.page) else {
            return false;
        };
        match key {
            "up" | "down" => self.move_action_cursor(key == "down", ctx),
            "n" | "p" if modifiers.control => self.move_action_cursor(key == "n", ctx),
            "escape" => effects.extend(self.menu_back(page, ctx)),
            "left" | "backspace" if page == ActionsPage::Devices && !modifiers.platform => {
                effects.extend(self.menu_back(page, ctx));
            }
            // Shift and Command with Enter are the direct paste shortcuts; they close the list
            // and fall through.
            "enter" if !modifiers.shift && !modifiers.platform && !modifiers.control => {
                let chosen = self
                    .actions_view(ctx)
                    .and_then(|view| view.rows.get(view.cursor).cloned());
                if let Some(row) = chosen.filter(|row| row.enabled) {
                    effects.extend(self.run_action(row.action, ctx));
                }
            }
            "enter" => {
                effects.extend(self.close_actions());
                return false;
            }
            _ => return false,
        }
        true
    }

    /// Back from the device page to the main page, or out of the list.
    fn menu_back(&mut self, page: ActionsPage, ctx: &Ctx) -> Effects {
        if page == ActionsPage::Devices {
            self.menu = self.menu.take().map(|menu| ActionMenu {
                page: ActionsPage::Main,
                cursor: 0,
                ..menu
            });
            self.reset_menu_cursor(ctx);
            vec![]
        } else {
            self.close_actions()
        }
    }
}
