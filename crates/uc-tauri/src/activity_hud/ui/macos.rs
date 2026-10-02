// Keep AppKit call sites consistent with the surrounding macOS adapters even
// when objc2 marks a particular SDK method as safe.
#![allow(unused_unsafe)]

//! Native macOS receive-activity HUD.
//!
//! Each receive task owns a titleless `NSPanel`. Multiple panels start as an
//! overlapping stack; clicking the top panel toggles the expanded vertical
//! list. Panels remain fixed in the system-managed upper-right position.
//!
//! The cancel control is a separate child panel anchored to the parent panel's
//! upper-left corner. It appears on hover, stays outside content layout, and
//! delegates cancellation through [`ActivityHudActions`].
//!
//! Listener callbacks can arrive on publisher threads, so all AppKit work is
//! dispatched through `AppHandle::run_on_main_thread`. Objective-C UI objects
//! stay in main-thread-local state and never cross thread boundaries.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::Arc;

use objc2::define_class;
use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyObject, NSObject, Sel};
use objc2::{msg_send, sel, AllocAnyThread, DefinedClass, MainThreadOnly};
use objc2_app_kit::{
    NSAccessibility, NSApplication, NSBackingStoreType, NSButton, NSCellImagePosition,
    NSClickGestureRecognizer, NSColor, NSControlSize, NSFont, NSGlassEffectView,
    NSGlassEffectViewStyle, NSImage, NSImageNameStopProgressFreestandingTemplate, NSImageScaling,
    NSImageSymbolConfiguration, NSImageSymbolScale, NSLayoutAttribute,
    NSLayoutConstraintOrientation, NSLineBreakMode, NSPanel, NSProgressIndicator,
    NSProgressIndicatorStyle, NSScreen, NSStackView, NSStackViewDistribution, NSTextField,
    NSTrackingArea, NSTrackingAreaOptions, NSUserInterfaceLayoutOrientation,
    NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSVisualEffectState, NSVisualEffectView,
    NSWindowAnimationBehavior, NSWindowOrderingMode, NSWindowStyleMask, NSWorkspace,
};
use objc2_foundation::{
    MainThreadMarker, NSEdgeInsets, NSOperatingSystemVersion, NSPoint, NSProcessInfo, NSRect,
    NSSize, NSString,
};
use tauri::AppHandle;
use tracing::{debug, warn};

use super::super::actions::ActivityHudActions;
use super::super::emitter::ActivityHudListener;
use super::super::state::{ActivityHudRow, RowState};

// 主线程独占的 HUD 内部状态。所有 ObjC 对象都只在主线程访问,
// `thread_local!` 防止其它线程偶然拿到。`RefCell` 在主线程内做可变
// 借用 —— run_on_main_thread 派发的闭包顺序执行,不会嵌套借用。
thread_local! {
    static HUD: RefCell<Option<HudInner>> = const { RefCell::new(None) };
}

struct HudInner {
    task_windows: HashMap<String, TaskWindow>,
    overflow_window: Option<OverflowWindow>,
    layout_signature: Vec<String>,
    expanded: bool,
}

struct TaskWindow {
    panel: Retained<NSPanel>,
    background: HudBackground,
    close_overlay: Retained<NSPanel>,
    filename_label: Retained<NSTextField>,
    subtitle_label: Retained<NSTextField>,
    progress_bar: Retained<NSProgressIndicator>,
    cancel_button: Retained<NSButton>,
    action_target: Retained<UCHudCancelButton>,
    _hover_target: Retained<UCHudHoverTarget>,
    _tracking_area: Retained<NSTrackingArea>,
    _overlay_tracking_area: Retained<NSTrackingArea>,
    toggle_recognizer: Retained<NSClickGestureRecognizer>,
    _toggle_target: Retained<UCHudStackToggleTarget>,
}

struct OverflowWindow {
    panel: Retained<NSPanel>,
    background: HudBackground,
    title: Retained<NSTextField>,
}

enum HudBackground {
    LiquidGlass(Retained<NSGlassEffectView>),
    Legacy(Retained<NSVisualEffectView>),
}

impl HudBackground {
    fn root_view(&self) -> &objc2_app_kit::NSView {
        match self {
            Self::LiquidGlass(glass) => glass,
            Self::Legacy(effect) => effect,
        }
    }

    fn set_content(&self, content: &objc2_app_kit::NSView) {
        match self {
            Self::LiquidGlass(glass) => unsafe {
                content.setTranslatesAutoresizingMaskIntoConstraints(false);
                glass.setContentView(Some(content));
            },
            Self::Legacy(effect) => attach_legacy_content(effect, content),
        }
    }

    fn add_tracking_area(&self, tracking_area: &NSTrackingArea) {
        unsafe {
            match self {
                Self::LiquidGlass(glass) => glass.addTrackingArea(tracking_area),
                Self::Legacy(effect) => effect.addTrackingArea(tracking_area),
            }
        }
    }

    fn add_gesture_recognizer(&self, recognizer: &NSClickGestureRecognizer) {
        unsafe {
            match self {
                Self::LiquidGlass(glass) => glass.addGestureRecognizer(recognizer),
                Self::Legacy(effect) => effect.addGestureRecognizer(recognizer),
            }
        }
    }

    fn apply_preferences(&self, preferences: AccessibilityPreferences) {
        if let Self::Legacy(effect) = self {
            apply_legacy_material_preferences(effect, preferences);
        }
    }
}

// ObjC 子类:NSButton 的 target/action 接收器。每行一个实例,持
// transfer_id + actions。
//
// 为什么用 ObjC 子类:NSControl.setTarget 接收的是 ObjC 对象指针,走
// selector dispatch。Rust 闭包没法直接挂上去。最干净的方法是定义一个
// 轻量 NSObject 子类,在它的 method 里调 Rust 逻辑。
struct UCHudCancelButtonIvars {
    transfer_id: String,
    entry_id: String,
    attempt_id: Option<String>,
    actions: Arc<dyn ActivityHudActions>,
    dismiss_only: Cell<bool>,
}

define_class!(
    // SAFETY:
    // - 父类 NSObject 没有子类约束。
    // - 本类不实现 Drop,objc2 会调用 ivars 的 drop_in_place,正常释放 Arc / String。
    #[unsafe(super(NSObject))]
    #[name = "UCHudCancelButton"]
    #[ivars = UCHudCancelButtonIvars]
    struct UCHudCancelButton;

    impl UCHudCancelButton {
        // selector 名固定 "cancelClicked:"。`_sender` 是 NSButton 自身,
        // 不需要用。
        #[unsafe(method(cancelClicked:))]
        fn cancel_clicked(&self, _sender: *mut AnyObject) {
            let ivars = self.ivars();
            let action = if ivars.dismiss_only.get() {
                "dismiss"
            } else {
                "cancel"
            };
            debug!(
                transfer_id = %ivars.transfer_id,
                user_action = action,
                "activity_hud action invoked"
            );
            if ivars.dismiss_only.get() {
                ivars.actions.dismiss(
                    &ivars.entry_id,
                    ivars.attempt_id.as_deref(),
                    &ivars.transfer_id,
                );
            } else {
                ivars.actions.cancel(
                    &ivars.entry_id,
                    ivars.attempt_id.as_deref(),
                    &ivars.transfer_id,
                );
            }
        }
    }
);

impl UCHudCancelButton {
    fn new(
        transfer_id: String,
        entry_id: String,
        attempt_id: Option<String>,
        actions: Arc<dyn ActivityHudActions>,
    ) -> Retained<Self> {
        // alloc 不需要主线程标记(本类继承 NSObject,不是 MainThreadOnly),
        // 但实际调用方都在主线程上 —— 没有限制就用 AllocAnyThread trait。
        let allocated: Allocated<Self> = Self::alloc();
        let this = allocated.set_ivars(UCHudCancelButtonIvars {
            transfer_id,
            entry_id,
            attempt_id,
            actions,
            dismiss_only: Cell::new(false),
        });
        unsafe { msg_send![super(this), init] }
    }

    fn set_close_action(&self, action: HudCloseAction) {
        self.ivars()
            .dismiss_only
            .set(matches!(action, HudCloseAction::Dismiss));
    }
}

struct UCHudHoverTargetIvars {
    overlay: Retained<NSPanel>,
    allowed: Cell<bool>,
    force_visible: Cell<bool>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "UCHudHoverTarget"]
    #[ivars = UCHudHoverTargetIvars]
    struct UCHudHoverTarget;

    impl UCHudHoverTarget {
        #[unsafe(method(mouseEntered:))]
        fn mouse_entered(&self, _event: *mut AnyObject) {
            if !self.ivars().allowed.get() {
                return;
            }
            unsafe {
                self.ivars()
                    .overlay
                    .setAlphaValue(hover_control_alpha(true));
                self.ivars().overlay.setIgnoresMouseEvents(false);
                self.ivars().overlay.orderFrontRegardless();
            }
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _event: *mut AnyObject) {
            if self.ivars().force_visible.get() {
                return;
            }
            unsafe {
                self.ivars()
                    .overlay
                    .setAlphaValue(hover_control_alpha(false));
                self.ivars().overlay.setIgnoresMouseEvents(true);
            }
        }
    }
);

impl UCHudHoverTarget {
    fn new(overlay: Retained<NSPanel>) -> Retained<Self> {
        let allocated: Allocated<Self> = Self::alloc();
        let this = allocated.set_ivars(UCHudHoverTargetIvars {
            overlay,
            allowed: Cell::new(true),
            force_visible: Cell::new(false),
        });
        unsafe { msg_send![super(this), init] }
    }

    fn set_allowed(&self, allowed: bool) {
        self.ivars().allowed.set(allowed);
        if !allowed {
            unsafe {
                self.ivars()
                    .overlay
                    .setAlphaValue(hover_control_alpha(false));
                self.ivars().overlay.setIgnoresMouseEvents(true);
            }
        }
    }

    fn set_force_visible(&self, force_visible: bool) {
        if self.ivars().force_visible.replace(force_visible) == force_visible {
            return;
        }
        if force_visible && self.ivars().allowed.get() {
            unsafe {
                self.ivars()
                    .overlay
                    .setAlphaValue(hover_control_alpha(true));
                self.ivars().overlay.setIgnoresMouseEvents(false);
                self.ivars().overlay.orderFrontRegardless();
            }
        } else if !force_visible {
            unsafe {
                self.ivars()
                    .overlay
                    .setAlphaValue(hover_control_alpha(false));
                self.ivars().overlay.setIgnoresMouseEvents(true);
            }
        }
    }
}

struct UCHudStackToggleTargetIvars;

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "UCHudStackToggleTarget"]
    #[ivars = UCHudStackToggleTargetIvars]
    struct UCHudStackToggleTarget;

    impl UCHudStackToggleTarget {
        #[unsafe(method(toggleStack:))]
        fn toggle_stack(&self, _sender: *mut AnyObject) {
            let Some(mtm) = MainThreadMarker::new() else {
                return;
            };
            HUD.with(|cell| {
                if let Some(inner) = cell.borrow_mut().as_mut() {
                    inner.toggle_expanded(mtm);
                }
            });
        }
    }
);

impl UCHudStackToggleTarget {
    fn new() -> Retained<Self> {
        let allocated: Allocated<Self> = Self::alloc();
        let this = allocated.set_ivars(UCHudStackToggleTargetIvars);
        unsafe { msg_send![super(this), init] }
    }
}

/// 实现 [`ActivityHudListener`],把 snapshot 派发到主线程渲染。
pub struct MacosActivityHudListener {
    app_handle: AppHandle,
    /// HUD 上的用户动作走这个 trait —— 平台 listener 不直接 import facade,
    /// 跟领域层解耦。装配代码在 `super::create_listener` 里把 actions
    /// 注入进来。
    actions: Arc<dyn ActivityHudActions>,
}

impl MacosActivityHudListener {
    pub fn new(app_handle: AppHandle, actions: Arc<dyn ActivityHudActions>) -> Self {
        Self {
            app_handle,
            actions,
        }
    }
}

impl ActivityHudListener for MacosActivityHudListener {
    fn on_changed(&self, snapshot: Vec<ActivityHudRow>) {
        let actions = Arc::clone(&self.actions);
        if let Err(err) = self.app_handle.run_on_main_thread(move || {
            // SAFETY: run_on_main_thread 的契约就是回调在主线程上执行。
            let mtm =
                MainThreadMarker::new().expect("AppHandle::run_on_main_thread callback 不在主线程");
            HUD.with(|cell| {
                let mut slot = cell.borrow_mut();
                if slot.is_none() {
                    *slot = Some(HudInner::create(mtm));
                }
                if let Some(inner) = slot.as_mut() {
                    inner.apply_snapshot(mtm, &snapshot, &actions);
                }
            });
        }) {
            warn!(error = %err, "activity_hud: run_on_main_thread 派发失败");
        }
    }
}

impl HudInner {
    fn create(_mtm: MainThreadMarker) -> Self {
        Self {
            task_windows: HashMap::new(),
            overflow_window: None,
            layout_signature: Vec::new(),
            expanded: false,
        }
    }

    fn apply_snapshot(
        &mut self,
        mtm: MainThreadMarker,
        snapshot: &[ActivityHudRow],
        actions: &Arc<dyn ActivityHudActions>,
    ) {
        debug!(
            row_count = snapshot.len(),
            "activity_hud: applying snapshot"
        );

        let task_keys: Vec<String> = snapshot.iter().map(task_key).collect();
        let task_key_refs: Vec<&str> = task_keys.iter().map(String::as_str).collect();
        let (visible_keys, overflow_count) = presentation_plan(&task_key_refs);
        let visible_key_set: std::collections::HashSet<&str> =
            visible_keys.iter().copied().collect();
        let rows_by_key: HashMap<String, &ActivityHudRow> =
            snapshot.iter().map(|row| (task_key(row), row)).collect();

        self.task_windows.retain(|key, window| {
            let keep = visible_key_set.contains(key.as_str());
            if !keep {
                window.hide();
            }
            keep
        });

        for key in &visible_keys {
            let Some(row) = rows_by_key.get(*key).copied() else {
                continue;
            };
            if let Some(window) = self.task_windows.get(*key) {
                window.update_from(row);
            } else {
                self.task_windows
                    .insert((*key).to_owned(), TaskWindow::create(mtm, row, actions));
            }
        }

        if overflow_count > 0 {
            if let Some(window) = self.overflow_window.as_ref() {
                window.update(overflow_count);
            } else {
                self.overflow_window = Some(OverflowWindow::create(mtm, overflow_count));
            }
        } else if let Some(window) = self.overflow_window.take() {
            window.hide();
        }

        let visible_window_count = visible_keys.len() + usize::from(overflow_count > 0);
        if visible_window_count <= 1 {
            self.expanded = false;
        }

        let mut next_layout_signature: Vec<String> =
            visible_keys.iter().map(|key| (*key).to_owned()).collect();
        if overflow_count > 0 {
            next_layout_signature.push("__overflow__".to_owned());
        }
        let current_refs: Vec<&str> = self.layout_signature.iter().map(String::as_str).collect();
        let next_refs: Vec<&str> = next_layout_signature.iter().map(String::as_str).collect();
        if should_relayout(&current_refs, &next_refs) {
            self.position_and_show(mtm, &visible_keys);
            self.layout_signature = next_layout_signature;
        }
        self.update_stack_interactions(&visible_keys, visible_window_count);
    }

    fn position_and_show(&self, mtm: MainThreadMarker, visible_keys: &[&str]) {
        let mut panels: Vec<Retained<NSPanel>> = visible_keys
            .iter()
            .filter_map(|key| {
                self.task_windows
                    .get(*key)
                    .map(|window| window.panel.clone())
            })
            .collect();
        if let Some(window) = self.overflow_window.as_ref() {
            panels.push(window.panel.clone());
        }

        let visible_frame = NSScreen::mainScreen(mtm)
            .map(|screen| screen.visibleFrame())
            .unwrap_or(NSRect::new(
                NSPoint::new(0.0, 0.0),
                NSSize::new(1_024.0, 768.0),
            ));
        let heights: Vec<f64> = panels.iter().map(|panel| panel_height(panel)).collect();
        let screen_top = visible_frame.origin.y + visible_frame.size.height;
        let origins = if self.expanded {
            stack_origins(screen_top, &heights)
        } else {
            collapsed_origins(screen_top, &heights)
        };
        let origin_x = visible_frame.origin.x + visible_frame.size.width - HUD_WIDTH - STACK_MARGIN;
        let animate_layout =
            should_animate_layout(current_accessibility_preferences().reduce_motion);

        for ((panel, height), origin_y) in panels.iter().zip(heights).zip(origins) {
            let frame = NSRect::new(
                NSPoint::new(origin_x, origin_y),
                NSSize::new(HUD_WIDTH, height),
            );
            unsafe {
                if panel.isVisible() && animate_layout {
                    panel.setFrame_display_animate(frame, true, true);
                } else {
                    panel.setFrame_display(frame, true);
                }
            }
        }
        for panel in panels.iter().rev() {
            unsafe {
                panel.orderFrontRegardless();
            }
        }
        for key in visible_keys {
            if let Some(window) = self.task_windows.get(*key) {
                window.anchor_close_overlay();
            }
        }
    }

    fn update_stack_interactions(&self, visible_keys: &[&str], visible_window_count: usize) {
        for (index, key) in visible_keys.iter().enumerate() {
            if let Some(window) = self.task_windows.get(*key) {
                window.set_stack_interaction(index == 0, self.expanded, visible_window_count > 1);
            }
        }
    }

    fn toggle_expanded(&mut self, mtm: MainThreadMarker) {
        let visible_window_count = self.layout_signature.len();
        let next = toggled_expanded(self.expanded, visible_window_count);
        if next == self.expanded {
            return;
        }
        self.expanded = next;
        let keys: Vec<String> = self
            .layout_signature
            .iter()
            .filter(|key| key.as_str() != "__overflow__")
            .cloned()
            .collect();
        let key_refs: Vec<&str> = keys.iter().map(String::as_str).collect();
        self.position_and_show(mtm, &key_refs);
        self.update_stack_interactions(&key_refs, visible_window_count);
    }
}

impl TaskWindow {
    fn create(
        mtm: MainThreadMarker,
        row: &ActivityHudRow,
        actions: &Arc<dyn ActivityHudActions>,
    ) -> Self {
        let (panel, background) = create_panel(mtm, 68.0);
        let filename_label =
            NSTextField::labelWithString(&NSString::from_str(&format_title(row)), mtm);
        unsafe {
            filename_label.setFont(Some(&NSFont::systemFontOfSize(NSFont::systemFontSize())));
            filename_label.setMaximumNumberOfLines(1);
            filename_label.setLineBreakMode(NSLineBreakMode::ByTruncatingMiddle);
            filename_label.setContentCompressionResistancePriority_forOrientation(
                249.0,
                NSLayoutConstraintOrientation::Horizontal,
            );
            filename_label.setTranslatesAutoresizingMaskIntoConstraints(false);
        }

        let subtitle_text = format_subtitle(row);
        let subtitle_label = NSTextField::labelWithString(&NSString::from_str(&subtitle_text), mtm);
        unsafe {
            let small_font_size = NSFont::smallSystemFontSize();
            subtitle_label.setFont(Some(&NSFont::systemFontOfSize(small_font_size)));
            subtitle_label.setTextColor(Some(&NSColor::secondaryLabelColor()));
            subtitle_label.setMaximumNumberOfLines(1);
            subtitle_label.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
            subtitle_label.setContentCompressionResistancePriority_forOrientation(
                249.0,
                NSLayoutConstraintOrientation::Horizontal,
            );
            subtitle_label.setTranslatesAutoresizingMaskIntoConstraints(false);
        }

        let progress_bar: Retained<NSProgressIndicator> = unsafe { NSProgressIndicator::new(mtm) };
        unsafe {
            progress_bar.setStyle(NSProgressIndicatorStyle::Bar);
            progress_bar.setIndeterminate(false);
            progress_bar.setMinValue(0.0);
            progress_bar.setMaxValue(1.0);
            progress_bar.setControlSize(NSControlSize::Small);
            progress_bar.setAccessibilityLabel(Some(&NSString::from_str("文件接收进度")));
            progress_bar.setTranslatesAutoresizingMaskIntoConstraints(false);
            let width_constraint = progress_bar
                .widthAnchor()
                .constraintEqualToConstant(content_width());
            width_constraint.setActive(true);
        }
        apply_progress_value(&progress_bar, row);

        let text_column: Retained<NSStackView> = unsafe { NSStackView::new(mtm) };
        unsafe {
            text_column.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
            text_column.setAlignment(NSLayoutAttribute::Leading);
            text_column.setSpacing(3.0);
            text_column.setDistribution(NSStackViewDistribution::Fill);
            text_column.setEdgeInsets(NSEdgeInsets {
                top: 10.0,
                left: 12.0,
                bottom: 10.0,
                right: 12.0,
            });
            text_column.setTranslatesAutoresizingMaskIntoConstraints(false);
            text_column
                .widthAnchor()
                .constraintEqualToConstant(HUD_WIDTH)
                .setActive(true);
            text_column.addArrangedSubview(&filename_label);
            text_column.addArrangedSubview(&subtitle_label);
            text_column.addArrangedSubview(&progress_bar);
            text_column.setHuggingPriority_forOrientation(
                249.0,
                NSLayoutConstraintOrientation::Horizontal,
            );
        }

        let cancel_target = UCHudCancelButton::new(
            row.transfer_id.clone(),
            row.entry_id.clone(),
            row.attempt_id.clone(),
            Arc::clone(actions),
        );
        cancel_target.set_close_action(close_action_for_state(&row.state));
        background.set_content(&text_column);
        let (close_overlay, cancel_button) = create_close_overlay(mtm, &cancel_target, row);
        let hover_target = UCHudHoverTarget::new(close_overlay.clone());
        let hover_owner: &AnyObject =
            unsafe { &*(&*hover_target as *const UCHudHoverTarget as *const AnyObject) };
        let tracking_area = create_tracking_area(hover_owner);
        let overlay_tracking_area = create_tracking_area(hover_owner);
        unsafe {
            if let Some(overlay_view) = close_overlay.contentView() {
                overlay_view.addTrackingArea(&overlay_tracking_area);
            }
            panel.addChildWindow_ordered(&close_overlay, NSWindowOrderingMode::Above);
        };
        background.add_tracking_area(&tracking_area);
        let toggle_target = UCHudStackToggleTarget::new();
        let toggle_target_obj: &AnyObject =
            unsafe { &*(&*toggle_target as *const UCHudStackToggleTarget as *const AnyObject) };
        let toggle_recognizer = unsafe {
            NSClickGestureRecognizer::initWithTarget_action(
                NSClickGestureRecognizer::alloc(mtm),
                Some(toggle_target_obj),
                Some(sel!(toggleStack:)),
            )
        };
        unsafe {
            toggle_recognizer.setNumberOfClicksRequired(1);
            toggle_recognizer.setButtonMask(1);
            toggle_recognizer.setDelaysPrimaryMouseButtonEvents(false);
            toggle_recognizer.setEnabled(false);
        }
        background.add_gesture_recognizer(&toggle_recognizer);

        Self {
            panel,
            background,
            close_overlay,
            filename_label,
            subtitle_label,
            progress_bar,
            cancel_button,
            action_target: cancel_target,
            _hover_target: hover_target,
            _tracking_area: tracking_area,
            _overlay_tracking_area: overlay_tracking_area,
            toggle_recognizer,
            _toggle_target: toggle_target,
        }
    }

    fn update_from(&self, row: &ActivityHudRow) {
        unsafe {
            self.filename_label
                .setStringValue(&NSString::from_str(&format_title(row)));
            self.subtitle_label
                .setStringValue(&NSString::from_str(&format_subtitle(row)));
        }
        apply_progress_value(&self.progress_bar, row);
        let close_action = close_action_for_state(&row.state);
        self.action_target.set_close_action(close_action);
        update_close_control(&self.cancel_button, row, close_action);
        self.background
            .apply_preferences(current_accessibility_preferences());
    }

    fn hide(&self) {
        unsafe {
            self.close_overlay.setAlphaValue(hover_control_alpha(false));
            self.close_overlay.setIgnoresMouseEvents(true);
            self.panel.orderOut(None);
        }
    }

    fn anchor_close_overlay(&self) {
        let frame = self.panel.frame();
        let (x, y) = close_overlay_origin(frame.origin.x, frame.origin.y, frame.size.height);
        unsafe {
            self.close_overlay.setFrameOrigin(NSPoint::new(x, y));
        }
    }

    fn set_stack_interaction(&self, is_top: bool, expanded: bool, has_multiple: bool) {
        let close_allowed = expanded || is_top;
        self._hover_target.set_allowed(close_allowed);
        let preferences = current_accessibility_preferences();
        self._hover_target.set_force_visible(
            close_allowed
                && close_control_visible(
                    false,
                    preferences.voice_over,
                    preferences.full_keyboard_access,
                ),
        );
        unsafe {
            self.panel.setMovable(false);
            self.toggle_recognizer.setEnabled(is_top && has_multiple);
        }
    }
}

impl OverflowWindow {
    fn create(mtm: MainThreadMarker, count: usize) -> Self {
        let (panel, background) = create_panel(mtm, 50.0);
        let title =
            NSTextField::labelWithString(&NSString::from_str(&format_overflow_title(count)), mtm);
        let label = NSTextField::labelWithString(&NSString::from_str("较早的传输仍在继续"), mtm);
        unsafe {
            title.setFont(Some(&NSFont::systemFontOfSize(NSFont::systemFontSize())));
            label.setFont(Some(&NSFont::systemFontOfSize(
                NSFont::smallSystemFontSize(),
            )));
            label.setTextColor(Some(&NSColor::secondaryLabelColor()));
        }
        let container: Retained<NSStackView> = unsafe { NSStackView::new(mtm) };
        unsafe {
            container.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
            container.setAlignment(NSLayoutAttribute::Leading);
            container.setSpacing(2.0);
            container.setEdgeInsets(NSEdgeInsets {
                top: 9.0,
                left: 12.0,
                bottom: 9.0,
                right: 12.0,
            });
            container.addArrangedSubview(&title);
            container.addArrangedSubview(&label);
        }
        background.set_content(&container);
        Self {
            panel,
            background,
            title,
        }
    }

    fn update(&self, count: usize) {
        unsafe {
            self.title
                .setStringValue(&NSString::from_str(&format_overflow_title(count)));
        }
        self.background
            .apply_preferences(current_accessibility_preferences());
    }

    fn hide(&self) {
        unsafe {
            self.panel.orderOut(None);
        }
    }
}

fn create_panel(mtm: MainThreadMarker, initial_height: f64) -> (Retained<NSPanel>, HudBackground) {
    let initial_rect = NSRect::new(
        NSPoint::new(0.0, 0.0),
        NSSize::new(HUD_WIDTH, initial_height),
    );
    let style = NSWindowStyleMask::Borderless
        | NSWindowStyleMask::NonactivatingPanel
        | NSWindowStyleMask::UtilityWindow
        | NSWindowStyleMask::HUDWindow;
    let panel = unsafe {
        NSPanel::initWithContentRect_styleMask_backing_defer(
            NSPanel::alloc(mtm),
            initial_rect,
            style,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    unsafe {
        panel.setLevel(objc2_app_kit::NSFloatingWindowLevel);
        panel.setFloatingPanel(true);
        panel.setBecomesKeyOnlyIfNeeded(true);
        panel.setHidesOnDeactivate(false);
        panel.setReleasedWhenClosed(false);
        panel.setOpaque(false);
        panel.setBackgroundColor(Some(&NSColor::clearColor()));
        panel.setHasShadow(false);
        panel.setAnimationBehavior(NSWindowAnimationBehavior::UtilityWindow);
        panel.setMovable(false);
        panel.setMovableByWindowBackground(false);
    }

    let background = create_background(mtm, 10.0, current_accessibility_preferences());
    unsafe {
        panel.setContentView(Some(background.root_view()));
    }
    (panel, background)
}

fn create_background(
    mtm: MainThreadMarker,
    corner_radius: f64,
    preferences: AccessibilityPreferences,
) -> HudBackground {
    match background_kind(
        supports_liquid_glass(),
        preferences.reduce_transparency,
        preferences.increase_contrast,
    ) {
        HudBackgroundKind::LiquidGlass => {
            let glass = unsafe { NSGlassEffectView::new(mtm) };
            unsafe {
                glass.setStyle(NSGlassEffectViewStyle::Regular);
                glass.setCornerRadius(corner_radius);
            }
            HudBackground::LiquidGlass(glass)
        }
        HudBackgroundKind::LegacyTranslucent | HudBackgroundKind::LegacyOpaque => {
            let effect = unsafe { NSVisualEffectView::new(mtm) };
            unsafe {
                effect.setState(NSVisualEffectState::Active);
                effect.setWantsLayer(true);
                if let Some(layer) = effect.layer() {
                    let _: () = msg_send![&layer, setCornerRadius: corner_radius];
                    let _: () = msg_send![&layer, setMasksToBounds: true];
                }
            }
            let background = HudBackground::Legacy(effect);
            background.apply_preferences(preferences);
            background
        }
    }
}

fn create_close_overlay(
    mtm: MainThreadMarker,
    cancel_target: &UCHudCancelButton,
    row: &ActivityHudRow,
) -> (Retained<NSPanel>, Retained<NSButton>) {
    let rect = NSRect::new(
        NSPoint::new(0.0, 0.0),
        NSSize::new(CLOSE_OVERLAY_SIZE, CLOSE_OVERLAY_SIZE),
    );
    let style = NSWindowStyleMask::Borderless
        | NSWindowStyleMask::NonactivatingPanel
        | NSWindowStyleMask::UtilityWindow;
    let overlay = unsafe {
        NSPanel::initWithContentRect_styleMask_backing_defer(
            NSPanel::alloc(mtm),
            rect,
            style,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    let background = create_background(
        mtm,
        CLOSE_OVERLAY_SIZE / 2.0,
        current_accessibility_preferences(),
    );
    let button: Retained<NSButton> = unsafe { NSButton::new(mtm) };
    unsafe {
        overlay.setLevel(objc2_app_kit::NSFloatingWindowLevel);
        overlay.setFloatingPanel(true);
        overlay.setBecomesKeyOnlyIfNeeded(true);
        overlay.setHidesOnDeactivate(false);
        overlay.setReleasedWhenClosed(false);
        overlay.setOpaque(false);
        overlay.setBackgroundColor(Some(&NSColor::clearColor()));
        overlay.setHasShadow(false);
        overlay.setAlphaValue(hover_control_alpha(false));
        overlay.setIgnoresMouseEvents(true);
        overlay.setContentView(Some(background.root_view()));

        button.setTitle(&NSString::new());
        button.setBordered(false);
        button.setControlSize(NSControlSize::Small);
        button.setImagePosition(NSCellImagePosition::ImageOnly);
        button.setImageScaling(NSImageScaling::ScaleNone);
        let symbol_name = NSString::from_str("xmark");
        let symbol_description = NSString::from_str("关闭");
        if let Some(image) = NSImage::imageWithSystemSymbolName_accessibilityDescription(
            &symbol_name,
            Some(&symbol_description),
        )
        .or_else(|| NSImage::imageNamed(NSImageNameStopProgressFreestandingTemplate))
        {
            let configuration =
                NSImageSymbolConfiguration::configurationWithScale(NSImageSymbolScale::Small);
            if let Some(configured) = image.imageWithSymbolConfiguration(&configuration) {
                button.setImage(Some(&configured));
            } else {
                button.setImage(Some(&image));
            }
        }
        let target_obj: &AnyObject =
            &*(cancel_target as *const UCHudCancelButton as *const AnyObject);
        button.setTarget(Some(target_obj));
        button.setAction(Some(sel!(cancelClicked:)));
        update_close_control(&button, row, close_action_for_state(&row.state));
    }
    background.set_content(&button);
    (overlay, button)
}

fn create_tracking_area(owner: &AnyObject) -> Retained<NSTrackingArea> {
    unsafe {
        NSTrackingArea::initWithRect_options_owner_userInfo(
            NSTrackingArea::alloc(),
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(0.0, 0.0)),
            NSTrackingAreaOptions::MouseEnteredAndExited
                | NSTrackingAreaOptions::ActiveAlways
                | NSTrackingAreaOptions::InVisibleRect,
            Some(owner),
            None,
        )
    }
}

fn attach_legacy_content(effect: &NSVisualEffectView, content: &objc2_app_kit::NSView) {
    unsafe {
        content.setTranslatesAutoresizingMaskIntoConstraints(false);
        effect.addSubview(content);
        for constraint in [
            content
                .topAnchor()
                .constraintEqualToAnchor(&effect.topAnchor()),
            content
                .leadingAnchor()
                .constraintEqualToAnchor(&effect.leadingAnchor()),
            content
                .trailingAnchor()
                .constraintEqualToAnchor(&effect.trailingAnchor()),
            content
                .bottomAnchor()
                .constraintEqualToAnchor(&effect.bottomAnchor()),
        ] {
            constraint.setActive(true);
        }
    }
}

fn panel_height(panel: &NSPanel) -> f64 {
    unsafe {
        panel
            .contentView()
            .map(|view| view.fittingSize().height.max(48.0))
            .unwrap_or(66.0)
    }
}

fn task_key(row: &ActivityHudRow) -> String {
    format!(
        "{}\u{1f}{}\u{1f}{}",
        row.entry_id,
        row.attempt_id.as_deref().unwrap_or_default(),
        row.transfer_id
    )
}

fn format_overflow_title(count: usize) -> String {
    format!("还有 {count} 个传输")
}

fn apply_progress_value(bar: &NSProgressIndicator, row: &ActivityHudRow) {
    unsafe {
        match progress_presentation(row.bytes_transferred, row.total_bytes, &row.state) {
            ProgressPresentation::Indeterminate => {
                bar.setIndeterminate(true);
                bar.startAnimation(None);
            }
            ProgressPresentation::Determinate(value) => {
                bar.stopAnimation(None);
                bar.setIndeterminate(false);
                bar.setDoubleValue(value);
            }
        }
    }
}

fn update_close_control(button: &NSButton, row: &ActivityHudRow, action: HudCloseAction) {
    let title = format_title(row);
    let (label, help) = match action {
        HudCloseAction::Cancel => (format!("取消传输 {title}"), "停止接收这个传输".to_owned()),
        HudCloseAction::Dismiss => (
            format!("关闭失败提示 {title}"),
            "关闭这个失败的传输提示".to_owned(),
        ),
        HudCloseAction::Disabled => (format!("传输状态 {title}"), "当前无法关闭".to_owned()),
    };
    unsafe {
        button.setEnabled(!matches!(action, HudCloseAction::Disabled));
        button.setToolTip(Some(&NSString::from_str(&label)));
        button.setAccessibilityLabel(Some(&NSString::from_str(&label)));
        button.setAccessibilityHelp(Some(&NSString::from_str(&help)));
    }
}

fn format_title(row: &ActivityHudRow) -> String {
    match &row.filenames {
        Some(names) if !names.is_empty() => {
            if names.len() == 1 {
                names[0].clone()
            } else {
                format!("{} 等 {} 项", names[0], names.len())
            }
        }
        _ => "正在接收文件…".to_string(),
    }
}

fn format_subtitle(row: &ActivityHudRow) -> String {
    match &row.state {
        RowState::Receiving => format_progress_subtitle(row),
        RowState::CancelPending => "取消中…".to_string(),
        RowState::Completed => "已完成".to_string(),
        RowState::Failed { reason } => match reason {
            Some(r) => format!("失败:{}", r),
            None => "失败".to_string(),
        },
        RowState::Cancelled { reason } => match reason {
            Some(r) => format!("已取消:{}", r),
            None => "已取消".to_string(),
        },
    }
}

fn format_progress_subtitle(row: &ActivityHudRow) -> String {
    let transferred = format_bytes(row.bytes_transferred);
    match (row.total_bytes, row.speed_bps) {
        (Some(total), Some(speed)) if speed > 0.0 => {
            let total_s = format_bytes(total);
            let speed_s = format_bytes(speed as u64);
            match row.eta_ms {
                Some(eta) => format!(
                    "{} / {} · {}/s · 剩 {}",
                    transferred,
                    total_s,
                    speed_s,
                    format_eta(eta)
                ),
                None => format!("{} / {} · {}/s", transferred, total_s, speed_s),
            }
        }
        (Some(total), _) => format!("{} / {}", transferred, format_bytes(total)),
        (None, _) => transferred,
    }
}

fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

fn format_eta(eta_ms: u64) -> String {
    let secs = eta_ms / 1_000;
    if secs >= 60 {
        format!("{} 分 {} 秒", secs / 60, secs % 60)
    } else if secs == 0 {
        "<1 秒".to_string()
    } else {
        format!("{} 秒", secs)
    }
}

// Sel 是从 objc2 sel! 宏生成的;留个 dead-code-suppress 防止某些 import
// 在缩减时被误删。
#[allow(dead_code)]
fn _keep_sel_import(_: Sel) {}

const MAX_VISIBLE_WINDOWS: usize = 4;
const HUD_WIDTH: f64 = 336.0;
const CLOSE_OVERLAY_SIZE: f64 = 16.0;
const STACK_MARGIN: f64 = 16.0;
const STACK_GAP: f64 = 8.0;
const COLLAPSED_OFFSET: f64 = 6.0;

fn content_width() -> f64 {
    HUD_WIDTH - 24.0
}

fn presentation_plan<'a>(task_ids: &'a [&'a str]) -> (Vec<&'a str>, usize) {
    let visible_task_count = if task_ids.len() > MAX_VISIBLE_WINDOWS {
        MAX_VISIBLE_WINDOWS - 1
    } else {
        task_ids.len()
    };
    let visible = task_ids
        .iter()
        .rev()
        .take(visible_task_count)
        .copied()
        .collect();
    (visible, task_ids.len() - visible_task_count)
}

fn stack_origins(screen_top: f64, heights: &[f64]) -> Vec<f64> {
    let mut next_top = screen_top - STACK_MARGIN;
    heights
        .iter()
        .map(|height| {
            let origin = next_top - height;
            next_top = origin - STACK_GAP;
            origin
        })
        .collect()
}

#[derive(Debug, PartialEq)]
enum ProgressPresentation {
    Indeterminate,
    Determinate(f64),
}

fn progress_presentation(
    bytes_transferred: u64,
    total_bytes: Option<u64>,
    state: &RowState,
) -> ProgressPresentation {
    if matches!(state, RowState::Completed) {
        return ProgressPresentation::Determinate(1.0);
    }
    if let Some(total) = total_bytes.filter(|total| *total > 0) {
        let fraction = (bytes_transferred as f64 / total as f64).clamp(0.0, 1.0);
        return ProgressPresentation::Determinate(fraction);
    }
    if matches!(state, RowState::Receiving | RowState::CancelPending) {
        ProgressPresentation::Indeterminate
    } else {
        ProgressPresentation::Determinate(0.0)
    }
}

fn hover_control_alpha(hovered: bool) -> f64 {
    if hovered {
        1.0
    } else {
        0.0
    }
}

fn should_relayout(current: &[&str], next: &[&str]) -> bool {
    current != next
}

fn close_overlay_origin(window_x: f64, window_y: f64, window_height: f64) -> (f64, f64) {
    (
        window_x - 8.0,
        window_y + window_height - CLOSE_OVERLAY_SIZE + 8.0,
    )
}

fn collapsed_origins(_screen_top: f64, _heights: &[f64]) -> Vec<f64> {
    let Some(first_height) = _heights.first() else {
        return Vec::new();
    };
    let first_origin = _screen_top - STACK_MARGIN - first_height;
    _heights
        .iter()
        .enumerate()
        .map(|(index, _)| first_origin - index as f64 * COLLAPSED_OFFSET)
        .collect()
}

fn toggled_expanded(expanded: bool, item_count: usize) -> bool {
    item_count > 1 && !expanded
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HudCloseAction {
    Cancel,
    Dismiss,
    Disabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HudBackgroundKind {
    LiquidGlass,
    LegacyTranslucent,
    LegacyOpaque,
}

fn background_kind(
    supports_liquid_glass: bool,
    reduce_transparency: bool,
    increase_contrast: bool,
) -> HudBackgroundKind {
    if supports_liquid_glass {
        HudBackgroundKind::LiquidGlass
    } else if uses_opaque_background(reduce_transparency, increase_contrast) {
        HudBackgroundKind::LegacyOpaque
    } else {
        HudBackgroundKind::LegacyTranslucent
    }
}

#[derive(Debug, Clone, Copy)]
struct AccessibilityPreferences {
    reduce_motion: bool,
    reduce_transparency: bool,
    increase_contrast: bool,
    voice_over: bool,
    full_keyboard_access: bool,
}

fn current_accessibility_preferences() -> AccessibilityPreferences {
    let workspace = NSWorkspace::sharedWorkspace();
    AccessibilityPreferences {
        reduce_motion: workspace.accessibilityDisplayShouldReduceMotion(),
        reduce_transparency: workspace.accessibilityDisplayShouldReduceTransparency(),
        increase_contrast: workspace.accessibilityDisplayShouldIncreaseContrast(),
        voice_over: workspace.isVoiceOverEnabled(),
        full_keyboard_access: MainThreadMarker::new()
            .is_some_and(|mtm| NSApplication::sharedApplication(mtm).isFullKeyboardAccessEnabled()),
    }
}

fn supports_liquid_glass() -> bool {
    NSProcessInfo::processInfo().isOperatingSystemAtLeastVersion(NSOperatingSystemVersion {
        majorVersion: 26,
        minorVersion: 0,
        patchVersion: 0,
    })
}

fn apply_legacy_material_preferences(
    effect: &NSVisualEffectView,
    preferences: AccessibilityPreferences,
) {
    unsafe {
        if uses_opaque_background(
            preferences.reduce_transparency,
            preferences.increase_contrast,
        ) {
            effect.setMaterial(NSVisualEffectMaterial::WindowBackground);
            effect.setBlendingMode(NSVisualEffectBlendingMode::WithinWindow);
        } else {
            effect.setMaterial(NSVisualEffectMaterial::Popover);
            effect.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
        }
    }
}

fn close_action_for_state(_state: &RowState) -> HudCloseAction {
    match _state {
        RowState::Receiving => HudCloseAction::Cancel,
        RowState::Failed { .. } => HudCloseAction::Dismiss,
        RowState::Completed | RowState::Cancelled { .. } | RowState::CancelPending => {
            HudCloseAction::Disabled
        }
    }
}

fn close_control_visible(hovered: bool, voice_over: bool, full_keyboard_access: bool) -> bool {
    hovered || voice_over || full_keyboard_access
}

fn should_animate_layout(reduce_motion: bool) -> bool {
    !reduce_motion
}

fn uses_opaque_background(reduce_transparency: bool, increase_contrast: bool) -> bool {
    reduce_transparency || increase_contrast
}

#[cfg(test)]
mod tests {
    use super::{
        background_kind, close_action_for_state, close_control_visible, close_overlay_origin,
        collapsed_origins, content_width, hover_control_alpha, presentation_plan,
        progress_presentation, should_animate_layout, should_relayout, stack_origins,
        toggled_expanded, uses_opaque_background, HudBackgroundKind, HudCloseAction,
        ProgressPresentation, RowState, CLOSE_OVERLAY_SIZE, HUD_WIDTH,
    };

    #[test]
    fn presentation_plan_orders_newest_task_first() {
        let task_ids = ["oldest", "middle", "newest"];

        let (visible, overflow) = presentation_plan(&task_ids);

        assert_eq!(visible, vec!["newest", "middle", "oldest"]);
        assert_eq!(overflow, 0);
    }

    #[test]
    fn presentation_plan_uses_the_fourth_window_for_overflow() {
        let task_ids = ["one", "two", "three", "four", "five"];

        let (visible, overflow) = presentation_plan(&task_ids);

        assert_eq!(visible, vec!["five", "four", "three"]);
        assert_eq!(overflow, 2);
    }

    #[test]
    fn presentation_plan_shows_all_four_tasks_without_overflow() {
        let task_ids = ["one", "two", "three", "four"];

        let (visible, overflow) = presentation_plan(&task_ids);

        assert_eq!(visible, vec!["four", "three", "two", "one"]);
        assert_eq!(overflow, 0);
    }

    #[test]
    fn stack_origins_place_windows_top_down_and_close_removed_gaps() {
        let origins = stack_origins(1_000.0, &[80.0, 64.0, 72.0]);
        assert_eq!(origins, vec![904.0, 832.0, 752.0]);

        let after_removal = stack_origins(1_000.0, &[80.0, 72.0]);
        assert_eq!(after_removal, vec![904.0, 824.0]);
    }

    #[test]
    fn unknown_total_is_indeterminate_only_while_transfer_is_active() {
        assert_eq!(
            progress_presentation(128, None, &RowState::Receiving),
            ProgressPresentation::Indeterminate
        );
        assert_eq!(
            progress_presentation(128, None, &RowState::Completed),
            ProgressPresentation::Determinate(1.0)
        );
    }

    #[test]
    fn known_total_uses_a_clamped_fraction() {
        assert_eq!(
            progress_presentation(150, Some(100), &RowState::Receiving),
            ProgressPresentation::Determinate(1.0)
        );
    }

    #[test]
    fn close_control_is_hidden_until_the_window_is_hovered() {
        assert_eq!(hover_control_alpha(false), 0.0);
        assert_eq!(hover_control_alpha(true), 1.0);
    }

    #[test]
    fn progress_updates_do_not_relayout_an_unchanged_stack() {
        let current = ["newest", "older"];
        assert!(!should_relayout(&current, &current));
        assert!(should_relayout(&current, &["new-task", "newest", "older"]));
        assert!(should_relayout(&current, &["newest"]));
    }

    #[test]
    fn close_component_is_anchored_to_the_window_upper_left() {
        assert_eq!(close_overlay_origin(100.0, 200.0, 80.0), (92.0, 272.0));
    }

    #[test]
    fn multiple_windows_default_to_an_overlapping_stack() {
        assert_eq!(
            collapsed_origins(1_000.0, &[80.0, 80.0, 80.0]),
            vec![904.0, 898.0, 892.0]
        );
    }

    #[test]
    fn clicking_the_top_window_toggles_expansion_only_for_multiple_items() {
        assert!(toggled_expanded(false, 2));
        assert!(!toggled_expanded(true, 2));
        assert!(!toggled_expanded(false, 1));
    }

    #[test]
    fn accessibility_preferences_override_hover_and_animation() {
        assert!(close_control_visible(false, true, false));
        assert!(close_control_visible(false, false, true));
        assert!(!close_control_visible(false, false, false));
        assert!(!should_animate_layout(true));
        assert!(should_animate_layout(false));
        assert!(uses_opaque_background(true, false));
        assert!(uses_opaque_background(false, true));
        assert!(!uses_opaque_background(false, false));
    }

    #[test]
    fn close_control_uses_compact_size_and_failed_rows_dismiss() {
        assert_eq!(CLOSE_OVERLAY_SIZE, 16.0);
        assert_eq!(
            close_action_for_state(&RowState::Receiving),
            HudCloseAction::Cancel
        );
        assert_eq!(
            close_action_for_state(&RowState::Failed { reason: None }),
            HudCloseAction::Dismiss
        );
        assert_eq!(
            close_action_for_state(&RowState::Completed),
            HudCloseAction::Disabled
        );
    }

    #[test]
    fn macos_26_uses_real_glass_and_older_systems_keep_accessible_fallbacks() {
        assert_eq!(
            background_kind(true, true, true),
            HudBackgroundKind::LiquidGlass
        );
        assert_eq!(
            background_kind(false, false, false),
            HudBackgroundKind::LegacyTranslucent
        );
        assert_eq!(
            background_kind(false, true, false),
            HudBackgroundKind::LegacyOpaque
        );
        assert_eq!(
            background_kind(false, false, true),
            HudBackgroundKind::LegacyOpaque
        );
    }

    #[test]
    fn window_and_content_widths_are_fixed_for_long_filenames() {
        assert_eq!(HUD_WIDTH, 336.0);
        assert_eq!(content_width(), 312.0);
    }
}
