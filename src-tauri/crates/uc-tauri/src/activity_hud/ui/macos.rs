// objc2 0.6 把大量 AppKit method 标成 safe;为了在跨版本升级时不需要逐
// 个补回 `unsafe { ... }`,以及与 `quick_panel/macos.rs` 的 unsafe 习惯保
// 持一致,这里 silence "unused_unsafe" warning。一旦发现某个 method 在
// 新版本下变成 unsafe,unsafe 块本来就准备好,不需要再加。
#![allow(unused_unsafe)]

//! macOS AppKit panel,渲染
//! [`ActivityHudState`](super::super::state::ActivityHudState) 的 snapshot。
//!
//! ## 设计要点
//!
//! ### 线程模型
//!
//! [`MacosActivityHudListener::on_changed`] 可能从任意 publisher 线程
//! (tokio worker 等) 进来,但所有 AppKit 调用必须在主线程。这里通过
//! Tauri 的 `AppHandle::run_on_main_thread` 把闭包派发到主线程 —— 与
//! `commands/quick_panel.rs` 等位置用的是同一套机制。
//!
//! ### 状态归属
//!
//! NSPanel / NSStackView / 行控件等 ObjC 对象不是 `Send + Sync`,无法
//! 放进 `Arc<Mutex<...>>` 跨线程持有。这里用 `thread_local!` 把 panel
//! 状态绑死在主线程上;listener 跨线程进来后,经 run_on_main_thread 派
//! 发后才访问 thread_local。
//!
//! ### 取消按钮
//!
//! 每行右侧一个 NSButton("✕")。点击 path:
//! 1. 调 [`ActivityHudActions::cancel`] —— actions 内部做乐观状态更新
//!    + 异步发出真正的取消请求
//! 2. UI 立即看到行变 "取消中…",几百 ms 内后端 status_changed 回流
//!    把行落到最终 `Cancelled`
//!
//! 按钮的 target 是一个自定义 ObjC 子类 `UCHudCancelButton`,继承自
//! NSObject,持有 transfer_id + actions 两个 ivars —— **不再直接持
//! emitter 或 facade**,平台模块跟领域代码完全解耦。
//!
//! ### 视觉
//!
//! - panel.styleMask: Borderless + NonactivatingPanel + Utility + HUDWindow
//! - contentView 是 NSVisualEffectView (material = HUDWindow,blending =
//!   BehindWindow) —— 毛玻璃跟系统通知中心 / AirDrop 同一观感
//! - 圆角 12pt (layer.cornerRadius + masksToBounds)
//! - panel.setOpaque(false) + setHasShadow(true)

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use objc2::define_class;
use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyObject, NSObject, Sel};
use objc2::{msg_send, sel, AllocAnyThread, DefinedClass, MainThreadOnly};
use objc2_app_kit::{
    NSBackingStoreType, NSBezelStyle, NSButton, NSColor, NSControlSize, NSFont, NSLayoutAttribute,
    NSLayoutConstraintOrientation, NSLineBreakMode, NSPanel, NSProgressIndicator,
    NSProgressIndicatorStyle, NSScreen, NSStackView, NSStackViewDistribution, NSTextField,
    NSUserInterfaceLayoutOrientation, NSVisualEffectBlendingMode, NSVisualEffectMaterial,
    NSVisualEffectState, NSVisualEffectView, NSWindowStyleMask,
};
use objc2_foundation::{MainThreadMarker, NSEdgeInsets, NSPoint, NSRect, NSSize, NSString};
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
    panel: Retained<NSPanel>,
    /// 行列表挂在这个 NSStackView 下(vertical)。
    stack: Retained<NSStackView>,
    /// transfer_id -> 对应行的视图聚合。
    rows: HashMap<String, RowView>,
}

struct RowView {
    /// 行根视图,水平方向:[text_column(filename + subtitle + progress) | cancel_btn]。
    container: Retained<NSStackView>,
    filename_label: Retained<NSTextField>,
    subtitle_label: Retained<NSTextField>,
    progress_bar: Retained<NSProgressIndicator>,
    cancel_button: Retained<NSButton>,
    /// 持有 ObjC target,保证 NSButton.target weak 引用还活着(NSButton
    /// 持有 target 是 weak,这里 strong 持一份避免被释放)。
    _cancel_target: Retained<UCHudCancelButton>,
}

// ObjC 子类:NSButton 的 target/action 接收器。每行一个实例,持
// transfer_id + actions。
//
// 为什么用 ObjC 子类:NSControl.setTarget 接收的是 ObjC 对象指针,走
// selector dispatch。Rust 闭包没法直接挂上去。最干净的方法是定义一个
// 轻量 NSObject 子类,在它的 method 里调 Rust 逻辑。
struct UCHudCancelButtonIvars {
    transfer_id: String,
    actions: Arc<dyn ActivityHudActions>,
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
            debug!(
                transfer_id = %ivars.transfer_id,
                "activity_hud: cancel button clicked"
            );
            ivars.actions.cancel(&ivars.transfer_id);
        }
    }
);

impl UCHudCancelButton {
    fn new(transfer_id: String, actions: Arc<dyn ActivityHudActions>) -> Retained<Self> {
        // alloc 不需要主线程标记(本类继承 NSObject,不是 MainThreadOnly),
        // 但实际调用方都在主线程上 —— 没有限制就用 AllocAnyThread trait。
        let allocated: Allocated<Self> = Self::alloc();
        let this = allocated.set_ivars(UCHudCancelButtonIvars {
            transfer_id,
            actions,
        });
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
    fn create(mtm: MainThreadMarker) -> Self {
        // panel 初始大小:宽 380、高 80。真正的高度由 contentView 的
        // autolayout 自动撑;这里只是给个起始 frame,后续 setFrame 调位置。
        let initial_rect = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(380.0, 80.0));

        // Borderless + HUDWindow + Utility:无标题栏的 HUD 浮窗外观。
        // NonactivatingPanel:点击 / 显示时不抢前台应用焦点 —— AirDrop 风格。
        let style = NSWindowStyleMask::Borderless
            | NSWindowStyleMask::NonactivatingPanel
            | NSWindowStyleMask::UtilityWindow
            | NSWindowStyleMask::HUDWindow;

        let panel: Retained<NSPanel> = unsafe {
            let alloc = NSPanel::alloc(mtm);
            NSPanel::initWithContentRect_styleMask_backing_defer(
                alloc,
                initial_rect,
                style,
                NSBackingStoreType::Buffered,
                false,
            )
        };

        unsafe {
            // 浮在普通窗口之上,主窗口失焦也保持可见。
            panel.setLevel(objc2_app_kit::NSFloatingWindowLevel);
            panel.setFloatingPanel(true);
            panel.setBecomesKeyOnlyIfNeeded(true);
            panel.setHidesOnDeactivate(false);
            // 不挂"closed"释放 —— 进程级 HUD,生命周期与 thread_local 同步。
            panel.setReleasedWhenClosed(false);
            // 透明 panel + 系统阴影 + 毛玻璃 contentView:这是 macOS HUD
            // 的标准三件套(AirDrop / 通知中心 / Spotlight 共享)。
            panel.setOpaque(false);
            panel.setBackgroundColor(Some(&NSColor::clearColor()));
            panel.setHasShadow(true);
        }

        // contentView:NSVisualEffectView 毛玻璃。圆角通过 layer 设。
        let effect: Retained<NSVisualEffectView> = unsafe { NSVisualEffectView::new(mtm) };
        unsafe {
            effect.setMaterial(NSVisualEffectMaterial::HUDWindow);
            effect.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
            effect.setState(NSVisualEffectState::Active);
            effect.setWantsLayer(true);
            if let Some(layer) = effect.layer() {
                let _: () = msg_send![&layer, setCornerRadius: 12.0_f64];
                let _: () = msg_send![&layer, setMasksToBounds: true];
            }
            effect.setTranslatesAutoresizingMaskIntoConstraints(false);
        }

        // 行列表 stack:挂在 effect 下,通过约束铺满。
        let stack: Retained<NSStackView> = unsafe { NSStackView::new(mtm) };
        unsafe {
            stack.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
            stack.setAlignment(NSLayoutAttribute::Leading);
            stack.setSpacing(10.0);
            stack.setDistribution(NSStackViewDistribution::Fill);
            stack.setEdgeInsets(NSEdgeInsets {
                top: 14.0,
                left: 16.0,
                bottom: 14.0,
                right: 16.0,
            });
            stack.setTranslatesAutoresizingMaskIntoConstraints(false);
            effect.addSubview(&stack);
            let constraints = [
                stack
                    .topAnchor()
                    .constraintEqualToAnchor(&effect.topAnchor()),
                stack
                    .leadingAnchor()
                    .constraintEqualToAnchor(&effect.leadingAnchor()),
                stack
                    .trailingAnchor()
                    .constraintEqualToAnchor(&effect.trailingAnchor()),
                stack
                    .bottomAnchor()
                    .constraintEqualToAnchor(&effect.bottomAnchor()),
            ];
            for c in &constraints {
                c.setActive(true);
            }
            // effect 作为 panel.contentView;NSWindow.setContentView 接收 NSView。
            panel.setContentView(Some(&effect));
        }

        Self {
            panel,
            stack,
            rows: HashMap::new(),
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

        // 1) 收集新 snapshot 里出现的 transfer_id —— 没在里头的现有行要移除。
        let new_ids: std::collections::HashSet<&str> =
            snapshot.iter().map(|r| r.transfer_id.as_str()).collect();
        let to_remove: Vec<String> = self
            .rows
            .keys()
            .filter(|k| !new_ids.contains(k.as_str()))
            .cloned()
            .collect();
        for id in to_remove {
            if let Some(row) = self.rows.remove(&id) {
                unsafe {
                    self.stack.removeArrangedSubview(&row.container);
                    // removeArrangedSubview 不会把视图从 superview 拿掉,需要手动。
                    row.container.removeFromSuperview();
                }
            }
        }

        // 2) 新增 / 更新行。
        for row in snapshot {
            if let Some(existing) = self.rows.get(&row.transfer_id) {
                existing.update_from(row);
            } else {
                let view = RowView::create(mtm, row, actions);
                unsafe {
                    self.stack.addArrangedSubview(&view.container);
                }
                self.rows.insert(row.transfer_id.clone(), view);
            }
        }

        // 3) 显示 / 隐藏 panel。
        if snapshot.is_empty() {
            unsafe {
                self.panel.orderOut(None);
            }
        } else {
            self.position_and_show(mtm);
        }
    }

    /// 把 panel 摆到主屏右上角并 orderFront。每次 snapshot 重新摆 ——
    /// snapshot 应用导致内容尺寸变化时,位置跟着变。
    fn position_and_show(&self, mtm: MainThreadMarker) {
        unsafe {
            let content_size = self.panel.contentView().map(|v| v.fittingSize());
            let target_size = content_size.unwrap_or(NSSize::new(380.0, 80.0));
            let width = 380.0_f64;
            let height = target_size.height.max(64.0);

            // 右上角:屏幕 visibleFrame 右上角内缩 16pt。
            let origin = match NSScreen::mainScreen(mtm) {
                Some(screen) => {
                    let vf = screen.visibleFrame();
                    NSPoint::new(
                        vf.origin.x + vf.size.width - width - 16.0,
                        vf.origin.y + vf.size.height - height - 16.0,
                    )
                }
                None => NSPoint::new(40.0, 40.0),
            };
            self.panel
                .setFrame_display(NSRect::new(origin, NSSize::new(width, height)), true);
            self.panel.orderFrontRegardless();
        }
    }
}

impl RowView {
    fn create(
        mtm: MainThreadMarker,
        row: &ActivityHudRow,
        actions: &Arc<dyn ActivityHudActions>,
    ) -> Self {
        // 文件名标签(主标题)。labelWithString 等价于 NSTextField 的
        // 标准 label 风格:无边框、无背景、不可编辑。
        let title_text = format_title(row);
        let filename_label = NSTextField::labelWithString(&NSString::from_str(&title_text), mtm);
        unsafe {
            filename_label.setFont(Some(&NSFont::systemFontOfSize(NSFont::systemFontSize())));
            filename_label.setMaximumNumberOfLines(1);
            filename_label.setLineBreakMode(NSLineBreakMode::ByTruncatingMiddle);
            filename_label.setTranslatesAutoresizingMaskIntoConstraints(false);
        }

        // 副标题:速度 / 进度 / 终态文案。
        let subtitle_text = format_subtitle(row);
        let subtitle_label = NSTextField::labelWithString(&NSString::from_str(&subtitle_text), mtm);
        unsafe {
            let small_font_size = NSFont::smallSystemFontSize();
            subtitle_label.setFont(Some(&NSFont::systemFontOfSize(small_font_size)));
            subtitle_label.setTextColor(Some(&NSColor::secondaryLabelColor()));
            subtitle_label.setMaximumNumberOfLines(1);
            subtitle_label.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
            subtitle_label.setTranslatesAutoresizingMaskIntoConstraints(false);
        }

        // 进度条。
        let progress_bar: Retained<NSProgressIndicator> = unsafe { NSProgressIndicator::new(mtm) };
        unsafe {
            progress_bar.setStyle(NSProgressIndicatorStyle::Bar);
            progress_bar.setIndeterminate(false);
            progress_bar.setMinValue(0.0);
            progress_bar.setMaxValue(1.0);
            progress_bar.setControlSize(NSControlSize::Small);
            progress_bar.setTranslatesAutoresizingMaskIntoConstraints(false);
            // 进度条横向铺满文本列宽度。
            let width_constraint = progress_bar
                .widthAnchor()
                .constraintGreaterThanOrEqualToConstant(280.0);
            width_constraint.setActive(true);
        }
        apply_progress_value(&progress_bar, row);

        // 文本列(vertical sub-stack):filename + subtitle + progress。
        let text_column: Retained<NSStackView> = unsafe { NSStackView::new(mtm) };
        unsafe {
            text_column.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
            text_column.setAlignment(NSLayoutAttribute::Leading);
            text_column.setSpacing(4.0);
            text_column.setDistribution(NSStackViewDistribution::Fill);
            text_column.setTranslatesAutoresizingMaskIntoConstraints(false);
            text_column.addArrangedSubview(&filename_label);
            text_column.addArrangedSubview(&subtitle_label);
            text_column.addArrangedSubview(&progress_bar);
            text_column.setHuggingPriority_forOrientation(
                249.0,
                NSLayoutConstraintOrientation::Horizontal,
            );
        }

        // 取消按钮:文本 "✕",bezelStyle 圆形小按钮。终态行的按钮设
        // disabled,update_from 里也会跟着切。
        let cancel_target = UCHudCancelButton::new(row.transfer_id.clone(), Arc::clone(actions));
        let cancel_button: Retained<NSButton> = unsafe { NSButton::new(mtm) };
        unsafe {
            cancel_button.setTitle(&NSString::from_str("✕"));
            cancel_button.setBezelStyle(NSBezelStyle::Circular);
            cancel_button.setControlSize(NSControlSize::Small);
            cancel_button.setTranslatesAutoresizingMaskIntoConstraints(false);
            // 固定按钮宽高,bezel circular 在小尺寸下需要明确约束。
            cancel_button
                .widthAnchor()
                .constraintEqualToConstant(22.0)
                .setActive(true);
            cancel_button
                .heightAnchor()
                .constraintEqualToConstant(22.0)
                .setActive(true);
            // 关联 target/action。target 是我们自定义 NSObject 子类的实例。
            // 所有 ObjC 对象 layout 一致(单 isa 指针起头),通过 raw
            // pointer 重解释把 `&UCHudCancelButton` 转成 `&AnyObject`。
            let target_obj: &AnyObject =
                &*(&*cancel_target as *const UCHudCancelButton as *const AnyObject);
            cancel_button.setTarget(Some(target_obj));
            cancel_button.setAction(Some(sel!(cancelClicked:)));
            apply_cancel_button_enabled(&cancel_button, row);
        }

        // 行总容器(horizontal):[text_column | cancel_button]。
        let container: Retained<NSStackView> = unsafe { NSStackView::new(mtm) };
        unsafe {
            container.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
            container.setAlignment(NSLayoutAttribute::CenterY);
            container.setSpacing(10.0);
            container.setDistribution(NSStackViewDistribution::Fill);
            container.setTranslatesAutoresizingMaskIntoConstraints(false);
            container.addArrangedSubview(&text_column);
            container.addArrangedSubview(&cancel_button);
        }

        Self {
            container,
            filename_label,
            subtitle_label,
            progress_bar,
            cancel_button,
            _cancel_target: cancel_target,
        }
    }

    /// 用新 snapshot 行原地更新本视图的文本/进度/按钮状态,不重建控件。
    fn update_from(&self, row: &ActivityHudRow) {
        unsafe {
            self.filename_label
                .setStringValue(&NSString::from_str(&format_title(row)));
            self.subtitle_label
                .setStringValue(&NSString::from_str(&format_subtitle(row)));
        }
        apply_progress_value(&self.progress_bar, row);
        apply_cancel_button_enabled(&self.cancel_button, row);
    }
}

fn apply_progress_value(bar: &NSProgressIndicator, row: &ActivityHudRow) {
    let value = match row.total_bytes {
        Some(total) if total > 0 => (row.bytes_transferred as f64 / total as f64).clamp(0.0, 1.0),
        _ => 0.0,
    };
    let final_value = match row.state {
        RowState::Completed => 1.0,
        // 失败/取消时定格在当时进度,视觉上"中断"。
        RowState::Failed { .. } | RowState::Cancelled { .. } => value,
        _ => value,
    };
    unsafe {
        bar.setDoubleValue(final_value);
    }
}

/// 终态行 / CancelPending 行的取消按钮 disabled —— 点击毫无意义。
fn apply_cancel_button_enabled(button: &NSButton, row: &ActivityHudRow) {
    let enabled = matches!(row.state, RowState::Receiving);
    unsafe {
        button.setEnabled(enabled);
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
