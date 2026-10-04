//! Opening and closing the panel, and what leaves it: pasting, entry actions and opening links.

use crate::actions;
use crate::ports::{EntryAction, PlatformError, ServiceError};
use crate::query::filters::Filters;
use crate::selection::Selection;

use super::{timing, Ctx, Effect, Effects, PanelState, Preview};

impl PanelState {
    /// The shortcut or the double tap: closes the panel if it is open, opens it otherwise.
    pub fn toggle(&mut self, ctx: &Ctx) -> Effects {
        if self.session.visible {
            vec![Effect::HideWindow]
        } else {
            self.show(ctx)
        }
    }

    /// Opens the panel afresh: nothing of the last time stays except what was loaded from the
    /// daemon, which is asked again.
    pub fn show(&mut self, ctx: &Ctx) -> Effects {
        self.search.filters = Filters::default();
        self.menu = None;
        self.search.disconnected = None;
        self.search.relaxations.clear();
        self.search.items.clear();
        self.search.selection = Selection::default();
        self.preview = Preview::default();
        self.search.grid_top = 0;
        self.search.hovered = None;
        self.search.keyboard = true;
        self.search.pointer_moved = false;
        self.session.message = None;
        self.suggest.open = false;
        self.suggest.cursor = 0;
        self.suggest.focused = false;
        self.session.visible = true;
        self.session.shown_at = ctx.now;
        let mut effects = vec![
            Effect::CaptureTarget,
            Effect::CancelReconnect,
            Effect::ClearImages,
            Effect::ClearImageBounds,
            Effect::ClearPreviewAnchor,
            Effect::Reposition,
            Effect::ResetInput,
            Effect::LoadOptions,
        ];
        effects.extend(self.search());
        effects.push(Effect::ApplyTheme);
        effects.push(Effect::ShowWindow);
        effects
    }

    /// The window is hidden now. Everything that only made sense while it was open stops.
    pub(super) fn on_hidden(&mut self) -> Effects {
        self.session.visible = false;
        self.menu = None;
        self.suggest.open = false;
        self.search.hovered = None;
        vec![
            Effect::ReturnFocus,
            Effect::CancelReconnect,
            // A hidden panel draws nothing, so it keeps no bitmaps; the next show loads them again.
            Effect::ClearImages,
            Effect::ClearImageBounds,
            Effect::ClearPreviewAnchor,
            Effect::CancelSearch,
            Effect::CancelPreview,
            Effect::HidePreviewWindow,
        ]
    }

    /// The panel window lost the focus.
    pub(super) fn on_deactivated(&mut self, ctx: &Ctx) -> Effects {
        if !self.session.visible
            || ctx.now.duration_since(self.session.shown_at) < timing::BLUR_IGNORE_AFTER_SHOW
        {
            return vec![];
        }
        vec![Effect::ScheduleBlurCheck(timing::BLUR_CHECK_DELAY)]
    }

    /// Closes the panel when neither of its windows has the focus any more.
    pub(super) fn on_blur_checked(
        &mut self,
        panel_active: bool,
        preview_active: bool,
        ctx: &Ctx,
    ) -> Effects {
        if !self.session.visible || panel_active {
            return vec![];
        }
        if self
            .session
            .blur_grace_until
            .is_some_and(|until| ctx.now < until)
        {
            return vec![];
        }
        if preview_active {
            return vec![];
        }
        vec![Effect::HideWindow]
    }

    /// Restores the selected entry to the clipboard, and pastes it into the target application
    /// when `paste` is set. Without automatic paste on this platform, the entry is only copied.
    pub fn restore(&mut self, paste: bool, plain: bool, keep_open: bool, ctx: &Ctx) -> Effects {
        if self.search.loading || self.session.busy {
            return vec![];
        }
        let Some(item) = self.active_item() else {
            return vec![];
        };
        if item.payload_state.as_deref() == Some("Lost") {
            self.session.message = Some(crate::text::t().service.entry_gone.into());
            return vec![];
        }
        let paste = paste && ctx.capabilities.auto_paste;
        if paste {
            if let Err(error) = ctx.target.check() {
                self.session.message = Some(error.to_string());
                return vec![];
            }
        }
        let id = item.entry_id.clone();
        self.session.busy = true;
        self.session.message = None;
        vec![Effect::Restore {
            id,
            plain,
            paste,
            keep_open,
        }]
    }

    pub(super) fn on_restored(
        &mut self,
        result: Result<(), ServiceError>,
        paste: bool,
        keep_open: bool,
        ctx: &Ctx,
    ) -> Effects {
        self.session.busy = false;
        if result.is_err() {
            self.session.message = Some(crate::text::t().service.copy_failed.into());
            return vec![];
        }
        if paste {
            if let Err(error) = ctx.target.check() {
                self.session.message = Some(error.to_string());
                return vec![];
            }
        }
        let mut effects = vec![];
        if !keep_open {
            effects.push(Effect::HideWindow);
        }
        if paste {
            if keep_open {
                self.session.blur_grace_until = Some(ctx.now + timing::BLUR_GRACE);
            }
            effects.push(Effect::PasteToTarget {
                keep_open,
                delay: timing::PASTE_DELAY,
            });
        }
        effects
    }

    /// Types the paths of a file entry into the target application.
    pub fn paste_paths(&mut self, paths: Vec<String>, ctx: &Ctx) -> Effects {
        let paths = paths
            .into_iter()
            .filter(|p| !p.is_empty())
            .collect::<Vec<_>>();
        if paths.is_empty() {
            self.session.message = Some(crate::text::t().service.no_paths.into());
            return vec![];
        }
        if let Err(error) = ctx.target.check() {
            self.session.message = Some(error.to_string());
            return vec![];
        }
        vec![
            Effect::HideWindow,
            Effect::TypeText {
                text: paths.join("\n"),
                delay: timing::PASTE_DELAY,
            },
        ]
    }

    /// The paste did not go through: the panel comes back with the reason.
    pub(super) fn on_paste_failed(&mut self, error: PlatformError, ctx: &Ctx) -> Effects {
        let effects = self.toggle(ctx);
        self.session.message = Some(error.to_string());
        effects
    }

    pub(super) fn on_paste_delivered(&mut self, keep_open: bool) -> Effects {
        // The target had to be in front to receive the paste; take the panel back.
        if keep_open && self.session.visible {
            vec![Effect::RaiseWindow]
        } else {
            vec![]
        }
    }

    /// An action on an entry other than restoring it.
    pub fn entry_action(&mut self, id: String, action: EntryAction) -> Effects {
        if self.search.loading || self.session.busy {
            return vec![];
        }
        self.session.busy = true;
        self.session.message = None;
        vec![Effect::EntryAction { id, action }]
    }

    pub(super) fn on_action_done(&mut self, result: Result<(), ServiceError>) -> Effects {
        self.session.busy = false;
        match result {
            Ok(()) => self.search(),
            Err(_) => {
                self.session.message = Some(crate::text::t().service.action_failed.into());
                vec![]
            }
        }
    }

    /// Command+O: opens the link in the browser or the file in its default application.
    pub fn open_selected(&mut self) -> Effects {
        let Some(target) = self.active_item().and_then(actions::openable) else {
            self.session.message = Some(crate::text::t().nothing_to_open.into());
            return vec![];
        };
        vec![Effect::OpenTarget(target)]
    }

    pub(super) fn on_opened(&mut self, result: Result<(), PlatformError>) -> Effects {
        match result {
            Ok(()) => vec![Effect::HideWindow],
            Err(error) => {
                self.session.message = Some(error.to_string());
                vec![]
            }
        }
    }

    /// Asks the GUI for something only it can do.
    pub fn ask_host(&mut self, request: super::HostRequest) -> Effects {
        vec![Effect::HostRequest(request)]
    }
}
