//! Keys and keyboard actions, handed to the state.
//!
//! The search box binds some keys as actions (copy, backspace, the arrows) that run before a plain
//! key listener, so the panel catches those actions itself.

use super::*;
use quick_panel_core::grid::Direction;

impl Panel {
    /// A key press anywhere in the panel window.
    pub(super) fn key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let data = self.ctx_data_editing(window, cx);
        let modifiers = event.keystroke.modifiers;
        let result = self.state.on_key(
            event.keystroke.key.as_str(),
            Modifiers {
                platform: modifiers.platform,
                control: modifiers.control,
                shift: modifiers.shift,
                alt: modifiers.alt,
            },
            &data.ctx(),
        );
        self.finish(result, window, cx);
    }

    /// Carries out the effects of a key and keeps it from anything else if the panel took it.
    fn finish(&mut self, result: KeyResult, window: &mut Window, cx: &mut Context<Self>) {
        if result.consumed {
            cx.stop_propagation();
        } else {
            cx.propagate();
        }
        self.run(result.effects, window, cx);
    }

    /// Tab and Shift+Tab.
    pub(super) fn tab(&mut self, reverse: bool, window: &mut Window, cx: &mut Context<Self>) {
        let data = self.ctx_data_editing(window, cx);
        let effects = self.state.tab(reverse, &data.ctx());
        self.run(effects, window, cx);
    }

    pub(super) fn copy_action(
        &mut self,
        _: &gpui_component::input::Copy,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let data = self.ctx_data_editing(window, cx);
        let result = self.state.on_copy(&data.ctx());
        self.finish(result, window, cx);
    }

    pub(super) fn backspace_action(
        &mut self,
        _: &gpui_component::input::Backspace,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let data = self.ctx_data(cx);
        let result = self.state.on_backspace(&data.ctx());
        self.finish(result, window, cx);
    }

    #[cfg(target_os = "macos")]
    pub(super) fn clear_action(
        &mut self,
        _: &gpui_component::input::DeleteToBeginningOfLine,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.clear_all(window, cx);
    }

    #[cfg(not(target_os = "macos"))]
    pub(super) fn clear_action(
        &mut self,
        _: &gpui_component::input::DeleteToPreviousWordStart,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.clear_all(window, cx);
    }

    fn clear_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let result = self.state.on_clear_all();
        if result.consumed {
            cx.stop_propagation();
        }
        self.run(result.effects, window, cx);
    }

    fn sideways(&mut self, direction: Direction, window: &mut Window, cx: &mut Context<Self>) {
        let data = self.ctx_data(cx);
        let result = self.state.on_sideways(direction, &data.ctx());
        if result.consumed {
            cx.stop_propagation();
        }
        self.run(result.effects, window, cx);
    }

    pub(super) fn move_left_action(
        &mut self,
        _: &gpui_component::input::MoveLeft,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.sideways(Direction::Left, window, cx);
    }

    pub(super) fn move_right_action(
        &mut self,
        _: &gpui_component::input::MoveRight,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.sideways(Direction::Right, window, cx);
    }
}
