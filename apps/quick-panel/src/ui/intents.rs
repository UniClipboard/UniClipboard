//! What the views ask of the panel: clicks, hovers and the calls of the preview window.

use super::*;
use quick_panel_core::actions::Action;

impl Panel {
    /// The shortcut or the double tap.
    pub fn toggle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.apply(window, cx, |state, ctx| state.toggle(ctx));
    }

    /// Closes the panel.
    pub(super) fn dismiss(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.run(vec![Effect::HideWindow], window, cx);
    }

    pub(super) fn search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.apply(window, cx, |state, _| state.search());
    }

    pub(super) fn clear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.apply(window, cx, |state, _| state.clear());
    }

    pub(super) fn select(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.apply(window, cx, |state, _| state.select(ix));
    }

    pub(super) fn restore(
        &mut self,
        paste: bool,
        plain: bool,
        keep_open: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply(window, cx, |state, ctx| {
            state.restore(paste, plain, keep_open, ctx)
        });
    }

    pub(super) fn open_actions(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.apply(window, cx, |state, ctx| state.open_actions(ctx));
    }

    /// Runs an action chosen in the action list.
    pub(super) fn run_action(
        &mut self,
        action: Action,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply(window, cx, |state, ctx| state.run_action(action, ctx));
    }

    pub(super) fn remove_filter(
        &mut self,
        dimension: Dimension,
        value: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply(window, cx, |state, _| state.remove_filter(dimension, value));
    }

    pub(super) fn hover(
        &mut self,
        ix: usize,
        while_loading: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply(window, cx, |state, _| state.hover(ix, while_loading));
    }

    pub(super) fn scroll_grid(&mut self, rows: isize, window: &mut Window, cx: &mut Context<Self>) {
        self.apply(window, cx, |state, _| state.scroll_grid(rows));
    }
}
