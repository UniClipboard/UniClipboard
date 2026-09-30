mod history;
mod image_preview;
mod preview_window;
mod view;

gpui::actions!(
    quick_panel,
    [NextSuggestion, PreviousSuggestion, NextCandidate]
);

use crate::{
    backend::{self, EntryAction},
    filters::{self, Dimension, Filters},
    platform::{self, PasteTarget},
    selection::Selection,
    strings,
};
use gpui::{
    prelude::*, Context, Entity, EntityInputHandler, Image, ImageFormat, KeyDownEvent,
    ScrollHandle, Subscription, Task, Window,
};
use gpui_component::input::{InputEvent, InputState};
use std::{
    collections::HashMap,
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};
use tracing::Instrument;
use uc_daemon_contract::api::{dto::search::SearchResultDto, types::SpaceMemberDto};
use uc_desktop::quick_panel_helper::HelperRequest;

#[derive(Clone)]
struct ImageData {
    image: Arc<Image>,
    width: u32,
    height: u32,
    size_bytes: i64,
}

#[derive(Default)]
struct PreviewState {
    entry: Option<String>,
    text: Option<String>,
    size: i64,
    loading: bool,
    expanded: bool,
    task: Option<Task<()>>,
}

/// The action list as the satellite window draws it.
#[derive(Clone, PartialEq)]
struct ActionsView {
    title: String,
    rows: Vec<crate::actions::Row>,
    cursor: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ActionsPage {
    Main,
    Devices,
}

/// The open action list. It acts on the selected entry.
struct ActionList {
    page: ActionsPage,
    cursor: usize,
    /// The preview was opened by the list and must be reloaded when the list closes.
    forced_preview: bool,
}

#[derive(Clone)]
struct PreviewSnapshot {
    item: Option<SearchResultDto>,
    text: Option<String>,
    image: Option<ImageData>,
    loading: bool,
    anchor: Option<crate::window_pair::PreviewAnchor>,
    scale: f64,
    actions: Option<ActionsView>,
    /// Name of the device the entry came from, when it is known.
    source_name: Option<String>,
    now_ms: i64,
}

impl PreviewSnapshot {
    /// An image is drawn full size, except while the action list replaces it.
    fn is_image(&self) -> bool {
        self.actions.is_none()
            && self
                .item
                .as_ref()
                .is_some_and(|item| item.content_type == "image")
    }
}

pub struct Panel {
    input: Entity<InputState>,
    items: Vec<SearchResultDto>,
    selection: Selection,
    scroll: ScrollHandle,
    runtime: tokio::runtime::Handle,
    target: Rc<PasteTarget>,
    loading: bool,
    busy: bool,
    message: Option<String>,
    locked: bool,
    total: u32,
    revision: u64,
    request: Option<Task<()>>,
    action_task: Option<Task<()>>,
    options_task: Option<Task<()>>,
    image_task: Option<Task<()>>,
    live_task: Option<Task<()>>,
    preview: PreviewState,
    preview_window: Option<gpui::AnyWindowHandle>,
    preview_anchor: Option<crate::window_pair::PreviewAnchor>,
    image_bounds: HashMap<String, gpui::Bounds<gpui::Pixels>>,
    /// First visible row of the image grid; the grid shows three rows from here.
    grid_top: usize,
    filters: Filters,
    actions: Option<ActionList>,
    /// Ways to loosen a search that found nothing, each with how many entries it would show.
    relaxations: Vec<(crate::states::Relaxation, Option<u32>)>,
    relax_cursor: usize,
    count_task: Option<Task<()>>,
    /// Failed attempts to reach the daemon since it last answered.
    disconnected: Option<u32>,
    reconnect_task: Option<Task<()>>,
    suggestions_open: bool,
    suggestion_cursor: usize,
    tags: Vec<String>,
    members: Vec<SpaceMemberDto>,
    images: HashMap<String, ImageData>,
    hovered: Option<usize>,
    keyboard: bool,
    pointer_moved: bool,
    visible: bool,
    shown_at: Instant,
    anchor: (f64, f64),
    scale: f64,
    cursor_anchored: bool,
    /// General settings from the last successful read. They are applied before every show, so the
    /// first frame already has the configured theme instead of waiting for a new request.
    general: Option<uc_daemon_contract::api::dto::settings::GeneralSettingsDto>,
    _subscriptions: Vec<Subscription>,
    blur_task: Option<Task<()>>,
}

impl Panel {
    pub fn new(
        runtime: tokio::runtime::Handle,
        target: Rc<PasteTarget>,
        message: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx
            .new(|cx| InputState::new(window, cx).placeholder(crate::strings::SEARCH_PLACEHOLDER));
        input.update(cx, |input, cx| input.focus(window, cx));
        let subscription = cx.subscribe_in(&input, window, |this, _, event, window, cx| {
            if let InputEvent::Change = event {
                this.input_changed(window, cx);
            }
        });
        let activation = cx.observe_window_activation(window, |this, window, cx| {
            if window.is_window_active() {
                this.blur_task = None;
                return;
            }
            if !this.visible || this.shown_at.elapsed() < Duration::from_millis(300) {
                return;
            }
            this.blur_task = Some(cx.spawn_in(window, async move |this, cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
                let _ = this.update_in(cx, |this, window, cx| {
                    this.dismiss_if_unfocused(window, cx);
                });
            }));
        });
        let bounds = window.bounds();
        let mut panel = Self {
            input,
            items: vec![],
            selection: Selection::default(),
            scroll: ScrollHandle::new(),
            runtime,
            target,
            loading: false,
            busy: false,
            message,
            locked: false,
            total: 0,
            revision: 0,
            request: None,
            action_task: None,
            options_task: None,
            image_task: None,
            live_task: None,
            preview: PreviewState::default(),
            preview_window: None,
            preview_anchor: None,
            image_bounds: HashMap::new(),
            grid_top: 0,
            filters: Filters::default(),
            actions: None,
            relaxations: vec![],
            relax_cursor: 0,
            count_task: None,
            disconnected: None,
            reconnect_task: None,
            suggestions_open: false,
            suggestion_cursor: 0,
            tags: filters::BUILTIN_TAGS.iter().map(|s| (*s).into()).collect(),
            members: vec![],
            images: HashMap::new(),
            hovered: None,
            keyboard: true,
            pointer_moved: false,
            visible: false,
            general: None,
            shown_at: Instant::now(),
            anchor: (f64::from(bounds.origin.x), f64::from(bounds.origin.y)),
            scale: std::env::var("UC_GPUI_SCALE")
                .ok()
                .and_then(|s| s.parse::<f64>().ok())
                .filter(|s| s.is_finite())
                .unwrap_or(1.)
                .clamp(0.8, 1.5),
            cursor_anchored: false,
            _subscriptions: vec![subscription, activation],
            blur_task: None,
        };
        panel.position(window, cx);
        panel.load_options(window, cx);
        panel.search(window, cx);
        panel.watch(window, cx);
        panel
    }

    pub fn toggle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.visible {
            self.dismiss(window, cx);
            return;
        }
        self.target = Rc::new(PasteTarget::capture());
        self.filters = Filters::default();
        self.actions = None;
        self.disconnected = None;
        self.reconnect_task = None;
        self.relaxations.clear();
        self.items.clear();
        self.selection.reset(0);
        self.preview = PreviewState::default();
        self.images.clear();
        self.image_bounds.clear();
        self.grid_top = 0;
        self.preview_anchor = None;
        self.hovered = None;
        self.keyboard = true;
        self.pointer_moved = false;
        self.message = None;
        self.suggestions_open = false;
        self.suggestion_cursor = 0;
        self.visible = true;
        self.shown_at = Instant::now();
        self.position(window, cx);
        self.input.update(cx, |input, cx| {
            input.set_value("", window, cx);
            input.focus(window, cx);
        });
        self.load_options(window, cx);
        self.search(window, cx);
        if crate::appearance::apply(self.general.as_ref(), window, cx).is_err() {
            tracing::warn!("Could not apply quick panel theme settings");
        }
        if let Err(message) = platform::set_visible(window, true) {
            self.message = Some(message);
        }
        cx.notify();
    }

    fn dismiss(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Err(message) = platform::set_visible(window, false) {
            self.message = Some(message);
            cx.notify();
            return;
        }
        self.visible = false;
        self.actions = None;
        self.reconnect_task = None;
        self.preview_anchor = None;
        self.request = None;
        self.preview.task = None;
        self.suggestions_open = false;
        self.hovered = None;
        self.hide_preview(cx);
        cx.notify();
    }

    fn hide_preview(&mut self, cx: &mut Context<Self>) {
        if let Some(handle) = self.preview_window {
            if handle
                .update(cx, |_, window, _| platform::set_visible(window, false))
                .is_err()
            {
                tracing::warn!("Preview window is unavailable");
            }
        }
    }

    fn dismiss_if_unfocused(&mut self, history: &mut Window, cx: &mut Context<Self>) {
        if !self.visible || history.is_window_active() {
            return;
        }
        let preview_focused = self
            .preview_window
            .and_then(|handle| {
                handle
                    .update(cx, |_, window, _| window.is_window_active())
                    .ok()
            })
            .unwrap_or(false);
        if !preview_focused {
            self.dismiss(history, cx);
        }
    }

    fn layout(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        gpui_component::Theme::global_mut(cx).font_size = gpui::px(16. * self.scale as f32);
        window.set_rem_size(gpui::px(16. * self.scale as f32));
        let width = crate::window_pair::PANEL_WIDTH * self.scale;
        let x = self.anchor.0;
        if let Err(message) = platform::set_frame(
            window,
            x,
            self.anchor.1,
            width,
            crate::window_pair::PANEL_HEIGHT * self.scale,
            cx,
        ) {
            self.message = Some(message);
        }
        if !self.preview.expanded {
            self.hide_preview(cx);
        }
    }

    fn position(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match platform::panel_anchor(
            self.cursor_anchored,
            crate::window_pair::PANEL_WIDTH * self.scale,
            crate::window_pair::PANEL_HEIGHT * self.scale,
        ) {
            Ok(anchor) => self.anchor = anchor,
            Err(message) => self.message = Some(message),
        }
        self.layout(window, cx);
    }

    fn show_preview(&mut self, history: &mut Window, cx: &mut Context<Self>) {
        self.update_preview_anchor(history, cx);
        let width =
            (crate::window_pair::PREVIEW_WIDTH + crate::window_pair::POINTER_DEPTH) * self.scale;
        let height = crate::window_pair::MIN_PREVIEW_HEIGHT * self.scale;
        if self.preview_window.is_none() {
            let snapshot = self.preview_snapshot();
            let panel = cx.entity();
            let history = history.window_handle();
            match cx.open_window(
                gpui::WindowOptions {
                    titlebar: None,
                    kind: gpui::WindowKind::PopUp,
                    focus: false,
                    show: false,
                    is_resizable: false,
                    window_background: gpui::WindowBackgroundAppearance::Transparent,
                    window_bounds: Some(gpui::WindowBounds::Windowed(gpui::Bounds::new(
                        gpui::point(
                            gpui::px(self.anchor.0 as f32),
                            gpui::px(self.anchor.1 as f32),
                        ),
                        gpui::size(gpui::px(width as f32), gpui::px(height as f32)),
                    ))),
                    ..Default::default()
                },
                |window, cx| {
                    let view = cx.new(|cx| {
                        preview_window::PreviewWindow::new(panel, history, snapshot, window, cx)
                    });
                    cx.new(|cx| gpui_component::Root::new(view, window, cx))
                },
            ) {
                Ok(handle) => self.preview_window = Some(handle.into()),
                Err(_) => {
                    self.message = Some("无法打开预览窗口。".into());
                }
            }
        }
    }

    fn update_preview_anchor(&mut self, history: &Window, cx: &mut Context<Self>) {
        let next = (|| {
            if !self.visible || !self.preview.expanded || self.loading {
                return None;
            }
            let id = self.preview.entry.as_ref()?;
            let index = self.items.iter().position(|item| &item.entry_id == id)?;
            let mut item = if self.filters.images_only() {
                *self.image_bounds.get(id)?
            } else {
                self.scroll.bounds_for_item(index)?
            };
            item.origin += self.scroll.offset();
            let viewport = self.scroll.bounds();
            let item = item.intersect(&viewport);
            if item.size.width <= gpui::px(0.) || item.size.height <= gpui::px(0.) {
                return None;
            }
            let frame = history.bounds();
            let screen = history.display(cx)?.bounds();
            use crate::window_pair::{PreviewAnchor, Rect};
            Some(PreviewAnchor {
                history: Rect {
                    x: f64::from(frame.origin.x),
                    y: f64::from(frame.origin.y),
                    width: f64::from(frame.size.width),
                    height: f64::from(frame.size.height),
                },
                item: Rect {
                    x: f64::from(frame.origin.x + item.origin.x),
                    y: f64::from(frame.origin.y + item.origin.y),
                    width: f64::from(item.size.width),
                    height: f64::from(item.size.height),
                },
                screen: Rect {
                    x: f64::from(screen.origin.x),
                    y: f64::from(screen.origin.y),
                    width: f64::from(screen.size.width),
                    height: f64::from(screen.size.height),
                },
                scale: self.scale,
            })
        })();
        if self.preview_anchor != next {
            self.preview_anchor = next;
            cx.notify();
        }
    }

    fn record_image_bounds(
        &mut self,
        id: String,
        mut bounds: gpui::Bounds<gpui::Pixels>,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        bounds.origin -= self.scroll.offset();
        self.image_bounds.insert(id, bounds);
        self.update_preview_anchor(window, cx);
    }

    fn preview_snapshot(&self) -> PreviewSnapshot {
        let item = self
            .items
            .iter()
            .find(|item| Some(&item.entry_id) == self.preview.entry.as_ref())
            .cloned();
        let image = item
            .as_ref()
            .and_then(|item| self.images.get(&item.entry_id))
            .cloned();
        let source_name = item.as_ref().and_then(|item| {
            let id = item.source_device.as_deref()?;
            self.members
                .iter()
                .find(|member| member.peer_id == id)
                .map(|member| member.device_name.clone())
        });
        PreviewSnapshot {
            item,
            image,
            text: self.preview.text.clone(),
            loading: self.preview.loading,
            anchor: self.preview_anchor,
            scale: self.scale,
            actions: self.actions_view(),
            source_name,
            now_ms: chrono::Utc::now().timestamp_millis(),
        }
    }

    /// The rows the list shows now: the entry's actions, or the devices to send to.
    fn action_rows(&self) -> Option<(String, Vec<crate::actions::Row>)> {
        let list = self.actions.as_ref()?;
        let item = self.active_index().and_then(|ix| self.items.get(ix))?;
        Some(match list.page {
            ActionsPage::Main => (
                strings::ACTIONS.to_string(),
                crate::actions::rows(item, self.target.name().as_deref()),
            ),
            ActionsPage::Devices => (
                strings::SEND_TO.to_string(),
                crate::actions::device_rows(&self.members),
            ),
        })
    }

    fn actions_view(&self) -> Option<ActionsView> {
        let (title, rows) = self.action_rows()?;
        let cursor = self
            .actions
            .as_ref()?
            .cursor
            .min(rows.len().saturating_sub(1));
        Some(ActionsView {
            title,
            rows,
            cursor,
        })
    }

    /// Command+K: shows what can be done with the selected entry in the satellite window.
    fn open_actions(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.loading || self.busy || self.actions.is_some() {
            return;
        }
        let Some(item) = self.active_index().and_then(|ix| self.items.get(ix)) else {
            return;
        };
        let id = item.entry_id.clone();
        let forced_preview = self.preview.entry.as_deref() != Some(&id);
        if forced_preview {
            self.preview.task = None;
            self.preview.entry = Some(id);
            self.preview.text = None;
            self.preview.loading = false;
        }
        self.preview.expanded = true;
        self.actions = Some(ActionList {
            page: ActionsPage::Main,
            cursor: 0,
            forced_preview,
        });
        if let Some((_, rows)) = self.action_rows() {
            if let Some(list) = self.actions.as_mut() {
                list.cursor = crate::actions::first_enabled(&rows);
            }
        }
        self.show_preview(window, cx);
        cx.notify();
    }

    fn close_actions(&mut self, window: &Window, cx: &mut Context<Self>) {
        let Some(list) = self.actions.take() else {
            return;
        };
        if list.forced_preview {
            // The preview text was never loaded for this entry; load it as if it had just been
            // selected.
            self.preview.entry = None;
            self.schedule_preview(window, cx);
        }
        cx.notify();
    }

    fn move_action_cursor(&mut self, forward: bool, cx: &mut Context<Self>) {
        let Some((_, rows)) = self.action_rows() else {
            return;
        };
        if let Some(list) = self.actions.as_mut() {
            list.cursor = crate::actions::step(&rows, list.cursor.min(rows.len() - 1), forward);
        }
        cx.notify();
    }

    /// Runs an action from the list, from a click or from Enter.
    fn run_action(
        &mut self,
        action: crate::actions::Action,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use crate::actions::Action;
        let Some(item) = self
            .active_index()
            .and_then(|ix| self.items.get(ix))
            .cloned()
        else {
            return;
        };
        if let Action::ChooseDevice = action {
            self.actions = self.actions.take().map(|list| ActionList {
                page: ActionsPage::Devices,
                cursor: 0,
                ..list
            });
            if let Some((_, rows)) = self.action_rows() {
                if let Some(list) = self.actions.as_mut() {
                    list.cursor = crate::actions::first_enabled(&rows);
                }
            }
            cx.notify();
            return;
        }
        // The list is closed first: pasting hides the panel, and the other actions leave it.
        self.close_actions(window, cx);
        let id = item.entry_id.clone();
        match action {
            Action::Paste => self.restore(true, false, false, window, cx),
            Action::PastePlain => self.restore(true, true, false, window, cx),
            Action::PasteKeepOpen => self.restore(true, false, true, window, cx),
            Action::Copy => self.restore(false, false, false, window, cx),
            Action::PastePaths => self.paste_paths(item.file_paths, window, cx),
            Action::Open => self.open_selected(window, cx),
            Action::RevealFile => {
                let path = item.file_paths.iter().find(|p| !p.is_empty());
                if let Some(Err(message)) = path.map(|p| platform::reveal_path(p)) {
                    self.message = Some(message);
                }
            }
            Action::OpenMainWindow => self.ask_host(HelperRequest::ShowMainWindow, window, cx),
            Action::OpenSettings => self.ask_host(HelperRequest::OpenSettings, window, cx),
            Action::Send(peer) => self.action(id, EntryAction::Send(peer), window, cx),
            Action::Favorite(value) => self.action(id, EntryAction::Favorite(value), window, cx),
            Action::Delete => self.action(id, EntryAction::Delete, window, cx),
            Action::ChooseDevice => {}
        }
        cx.notify();
    }

    /// Command+O: opens the link in the browser or the file in its default application.
    fn open_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(target) = self
            .active_index()
            .and_then(|ix| self.items.get(ix))
            .and_then(crate::actions::openable)
        else {
            self.message = Some(strings::NOTHING_TO_OPEN.into());
            cx.notify();
            return;
        };
        match platform::open_target(&target) {
            Ok(()) => self.dismiss(window, cx),
            Err(message) => self.message = Some(message),
        }
        cx.notify();
    }

    /// Applies the persisted double-tap modifier. Starting the worker can block briefly, so it
    /// runs off the UI thread; a failure (for example no Accessibility permission) only means the
    /// trigger is unavailable, and the keyboard shortcut keeps working.
    fn apply_double_tap_modifier(
        &self,
        modifier: uc_daemon_contract::api::dto::settings::QuickPanelDoubleTapModifierDto,
        cx: &mut Context<Self>,
    ) {
        let monitor = cx.global::<crate::modifier_keys::DoubleTap>().0.clone();
        self.runtime.spawn_blocking(move || {
            if monitor.set_modifier(modifier).is_err() {
                tracing::warn!("Modifier double-tap trigger is unavailable");
            }
        });
    }

    fn load_options(&mut self, window: &Window, cx: &mut Context<Self>) {
        let runtime = self.runtime.clone();
        self.options_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = runtime
                .spawn(
                    async { backend::options().await }
                        .instrument(tracing::info_span!("gpui.options")),
                )
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                match result {
                    Ok(Ok(options)) => {
                        if let Some(settings)=&options.settings{
                            this.cursor_anchored=matches!(settings.quick_panel.position,uc_daemon_contract::api::dto::settings::QuickPanelPositionDto::FollowCursor);
                            if cx.global_mut::<crate::shortcuts::Shortcuts>().configure(settings).is_err(){this.message=Some("无法注册快捷键，可能已被其他程序占用。".into());}
                            this.apply_double_tap_modifier(settings.quick_panel.double_tap_modifier, cx);
                        }
                        if let Some(settings) = &options.settings {
                            this.general = Some(settings.general.clone());
                        }
                        if crate::appearance::apply(
                            options.settings.as_ref().map(|s| &s.general),
                            window,
                            cx,
                        )
                        .is_err()
                        {
                            tracing::warn!("Could not apply quick panel theme settings");
                        }
                        this.tags = options.tags;
                        this.members = options.members;
                    }
                    _ => tracing::warn!("Could not load quick panel filter options"),
                }
                cx.notify();
            });
        }));
    }

    fn watch(&mut self, window: &Window, cx: &mut Context<Self>) {
        let runtime = self.runtime.clone();
        self.live_task = Some(cx.spawn_in(window, async move |this, cx| loop {
            let (send, mut receive) = tokio::sync::mpsc::channel(8);
            runtime.spawn(
                backend::watch_changes(send).instrument(tracing::info_span!("gpui.realtime")),
            );
            while let Some(first) = receive.recv().await {
                let mut event = first;
                if event == backend::Live::Changed {
                    cx.background_executor()
                        .timer(Duration::from_millis(120))
                        .await;
                    // Several changes are one search, but a lock among them must win.
                    while let Ok(next) = receive.try_recv() {
                        if next != backend::Live::Changed {
                            event = next;
                        }
                    }
                }
                if this
                    .update_in(cx, |this, window, cx| match event {
                        backend::Live::ContentLocked => this.drop_content(cx),
                        backend::Live::Changed | backend::Live::ContentUnlocked => {
                            if this.visible && !this.busy {
                                this.search(window, cx);
                            }
                        }
                    })
                    .is_err()
                {
                    return;
                }
            }
            cx.background_executor().timer(Duration::from_secs(2)).await;
        }));
    }

    /// The daemon says content is locked. Everything derived from history goes at once, shown or
    /// not: rows, thumbnails, previews, the action list and the names of tags and devices. A
    /// search that was already running is discarded by bumping the revision.
    fn drop_content(&mut self, cx: &mut Context<Self>) {
        tracing::warn!("Quick panel dropped its content: content is locked");
        self.revision += 1;
        self.request = None;
        self.loading = false;
        self.locked = true;
        self.disconnected = None;
        self.reconnect_task = None;
        self.actions = None;
        self.items.clear();
        self.total = 0;
        self.selection.reset(0);
        self.images.clear();
        self.image_bounds.clear();
        self.grid_top = 0;
        self.preview = PreviewState::default();
        self.preview_anchor = None;
        self.relaxations.clear();
        self.tags.clear();
        self.members.clear();
        self.message = None;
        self.hovered = None;
        self.hide_preview(cx);
        cx.notify();
    }

    fn input_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.filters.query = self.input.read(cx).value().to_string();
        self.close_actions(window, cx);
        self.suggestions_open = !self.filters.query.is_empty();
        self.suggestion_cursor = 0;
        self.hovered = None;
        self.keyboard = true;
        self.search(window, cx);
    }

    fn search(&mut self, window: &Window, cx: &mut Context<Self>) {
        self.run_search(false, window, cx);
    }

    /// A search. A silent one keeps the page on screen while it runs, for retries by the panel
    /// itself.
    fn run_search(&mut self, silent: bool, window: &Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.revision += 1;
        let revision = self.revision;
        self.loading = !silent;
        self.message = None;
        self.hovered = None;
        self.preview.task = None;
        let filters = self.filters.clone();
        let runtime = self.runtime.clone();
        self.request = Some(cx.spawn_in(window, async move |this,cx| {
            if !filters.query.trim().is_empty() { cx.background_executor().timer(Duration::from_millis(300)).await; }
            let (mut send, receive) = tokio::sync::oneshot::channel();
            runtime.spawn(async move {
                tracing::debug!("Quick panel query started");
                let result = tokio::select! { result = backend::search_filtered(filters) => result, _ = send.closed() => return };
                if result.is_err() { tracing::warn!("Quick panel query failed"); }
                let _ = send.send(result);
            }.instrument(tracing::info_span!("gpui.search", request_id=revision)));
            let result = receive.await.unwrap_or_else(|_| Err(backend::SearchFailure::Unavailable("后台请求已中断，请重试。".into())));
            let _ = this.update_in(cx, |this,window,cx| {
                if this.revision != revision { return; }
                this.loading = false;
                match result {
                    Ok(result) => {
                        this.locked = false; this.disconnected = None; this.reconnect_task = None;
                        this.total = result.total; this.items = result.items;this.image_bounds.clear();this.grid_top = 0;
                        this.selection.reset(this.items.len()); this.scroll.set_offset(gpui::point(gpui::px(0.),gpui::px(0.)));
                        this.schedule_preview(window,cx);this.load_images(window,cx);
                    }
                    Err(error) => {
                        this.locked = matches!(error,backend::SearchFailure::Locked);
                        // Locked and disconnected have pages of their own instead of a message.
                        let own_page = this.locked || matches!(error, backend::SearchFailure::Disconnected);
                        this.message = (!own_page).then(|| error.to_string());
                        if matches!(error, backend::SearchFailure::Disconnected) {
                            this.disconnected = Some(this.disconnected.unwrap_or(0) + 1);
                            this.schedule_reconnect(window, cx);
                        } else {
                            this.disconnected = None;
                            // A locked history is unlocked in the main window; look again soon.
                            if this.locked { this.schedule_reconnect(window, cx); }
                        }
                        this.items.clear(); this.total = 0; this.selection.reset(0);
                    }
                }
                this.relaxations.clear();
                this.count_task = None;
                if this.items.is_empty() && this.message.is_none() && !this.locked && this.disconnected.is_none() {
                    this.load_relaxations(window, cx);
                }
                if this.items.is_empty() { this.preview = PreviewState::default(); this.layout(window,cx); }
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// Asks the GUI for something only it can do, and gets out of its way.
    fn ask_host(&mut self, request: HelperRequest, window: &mut Window, cx: &mut Context<Self>) {
        match crate::host::send(request) {
            Ok(()) => self.dismiss(window, cx),
            Err(message) => {
                self.message = Some(message);
                cx.notify();
            }
        }
    }

    fn device_name(&self, id: &str) -> String {
        self.members
            .iter()
            .find(|member| member.peer_id == id)
            .map_or_else(|| id.to_string(), |member| member.device_name.clone())
    }

    /// Tries the daemon again a few seconds after it did not answer, for as long as the panel is
    /// open. The attempt count is shown on the disconnected page.
    fn schedule_reconnect(&mut self, window: &Window, cx: &mut Context<Self>) {
        self.reconnect_task = Some(cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(3)).await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this.visible && (this.disconnected.is_some() || this.locked) {
                    this.run_search(true, window, cx);
                }
            });
        }));
    }

    /// Works out how many entries each way of loosening the search would show.
    fn load_relaxations(&mut self, window: &Window, cx: &mut Context<Self>) {
        let options = crate::states::relaxations(&self.filters, |id| self.device_name(id));
        self.relax_cursor = 0;
        if options.is_empty() {
            return;
        }
        let queries: Vec<Filters> = options.iter().map(|o| o.filters.clone()).collect();
        self.relaxations = options.into_iter().map(|o| (o, None)).collect();
        let revision = self.revision;
        let runtime = self.runtime.clone();
        self.count_task = Some(cx.spawn_in(window, async move |this, cx| {
            let mut totals = vec![];
            for filters in queries {
                totals.push(runtime.spawn(backend::count(filters)).await.ok().flatten());
            }
            let _ = this.update(cx, |this, cx| {
                if this.revision != revision {
                    return;
                }
                for ((_, count), total) in this.relaxations.iter_mut().zip(totals) {
                    *count = total;
                }
                cx.notify();
            });
        }));
    }

    /// The suggestions worth showing: those that would find something, or are still being counted.
    fn visible_relaxations(&self) -> Vec<&(crate::states::Relaxation, Option<u32>)> {
        self.relaxations
            .iter()
            .filter(|(_, count)| *count != Some(0))
            .collect()
    }

    fn apply_relaxation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((relaxation, _)) = self
            .visible_relaxations()
            .get(self.relax_cursor)
            .map(|entry| (*entry).clone())
        else {
            return;
        };
        self.filters = relaxation.filters;
        self.suggestions_open = false;
        self.hovered = None;
        self.keyboard = true;
        self.search(window, cx);
    }

    fn active_index(&self) -> Option<usize> {
        self.hovered.or(self.selection.selected())
    }

    fn load_images(&mut self, window: &Window, cx: &mut Context<Self>) {
        let ids = self
            .items
            .iter()
            .filter(|item| {
                item.content_type == "image" && !self.images.contains_key(&item.entry_id)
            })
            .map(|item| item.entry_id.clone())
            .collect::<Vec<_>>();
        if ids.is_empty() {
            return;
        }
        let runtime = self.runtime.clone();
        self.image_task=Some(cx.spawn_in(window,async move|this,cx|{
            let (send,mut receive)=tokio::sync::mpsc::channel(4);
            runtime.spawn(async move{
                let mut tasks=tokio::task::JoinSet::new();let mut ids=ids.into_iter();
                loop{
                    while tasks.len()<4{
                        let Some(id)=ids.next()else{break;};
                        tasks.spawn(async move{let result=backend::preview(id.clone(),"image".into()).await;(id,result)}.in_current_span());
                    }
                    if tasks.is_empty(){break;}
                    tokio::select!{
                        result=tasks.join_next()=>{if let Some(Ok(result))=result{if send.send(result).await.is_err(){break;}}},
                        _=send.closed()=>break,
                    }
                }
            }.instrument(tracing::info_span!("gpui.thumbnails")));
            while let Some((id,result))=receive.recv().await{
                let _=this.update(cx,|this,cx|{
                    if let Ok(data)=result{if let Some((bytes,mime,width,height))=data.image{if let Some(format)=ImageFormat::from_mime_type(&mime){
                        this.images.insert(id,ImageData{image:Arc::new(Image::from_bytes(format,bytes)),width,height,size_bytes:data.size});
                    }}}cx.notify();
                });
            }
        }));
    }

    fn schedule_preview(&mut self, window: &Window, cx: &mut Context<Self>) {
        let Some(item) = self
            .active_index()
            .and_then(|ix| self.items.get(ix))
            .cloned()
        else {
            return;
        };
        if self.preview.entry.as_deref() == Some(&item.entry_id) {
            return;
        }
        let delay = if self.preview.expanded { 120 } else { 500 };
        let runtime = self.runtime.clone();
        self.preview.task = Some(cx.spawn_in(window, async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(delay))
                .await;
            let id = item.entry_id.clone();
            let _ = this.update_in(cx, |this, window, cx| {
                if !this.visible {
                    return;
                }
                this.preview.entry = Some(id.clone());
                this.preview.text = None;
                this.preview.loading = true;
                this.preview.expanded = true;
                this.show_preview(window, cx);
                cx.notify();
            });
            let result = runtime
                .spawn(
                    async move { backend::preview(item.entry_id, item.content_type).await }
                        .instrument(tracing::info_span!("gpui.preview")),
                )
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.preview.entry.as_deref() != Some(&id) {
                    return;
                }
                this.preview.loading = false;
                match result {
                    Ok(Ok(data)) => {
                        this.preview.text = data.text;
                        this.preview.size = data.size;
                        if let Some((bytes, mime, width, height)) = data.image {
                            if let Some(format) = ImageFormat::from_mime_type(&mime) {
                                this.images.insert(
                                    id,
                                    ImageData {
                                        image: Arc::new(Image::from_bytes(format, bytes)),
                                        width,
                                        height,
                                        size_bytes: data.size,
                                    },
                                );
                            }
                        }
                    }
                    _ => this.preview.text = Some("无法读取预览，请重试。".into()),
                }
                cx.notify();
            });
        }));
    }

    fn select(&mut self, ix: usize, window: &Window, cx: &mut Context<Self>) {
        if self.loading || self.busy {
            return;
        }
        self.selection.index = ix;
        self.hovered = None;
        self.keyboard = true;
        if self.filters.images_only() {
            self.grid_top = crate::grid::first_row_for(ix, self.grid_top, self.items.len());
        } else {
            self.scroll.scroll_to_item(ix);
        }
        self.schedule_preview(window, cx);
        cx.notify();
    }

    fn restore(
        &mut self,
        paste: bool,
        plain: bool,
        keep_open: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.loading || self.busy {
            return;
        }
        let Some(item) = self.active_index().and_then(|ix| self.items.get(ix)) else {
            return;
        };
        if item.payload_state.as_deref() == Some("Lost") {
            self.message = Some("内容已不可用。".into());
            cx.notify();
            return;
        }
        if paste {
            if let Err(message) = self.target.check() {
                self.message = Some(message);
                cx.notify();
                return;
            }
        }
        let id = item.entry_id.clone();
        let runtime = self.runtime.clone();
        let target = self.target.clone();
        self.busy = true;
        self.message = None;
        self.action_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = runtime
                .spawn(
                    async move { backend::restore_with_options(id, plain).await }
                        .instrument(tracing::info_span!("gpui.restore")),
                )
                .await;
            let proceed = this
                .update_in(cx, |this, window, cx| {
                    this.busy = false;
                    match result {
                        Ok(Ok(())) => {
                            if paste {
                                if let Err(message) = target.check() {
                                    this.message = Some(message);
                                    cx.notify();
                                    return false;
                                }
                            }
                            if !keep_open {
                                this.dismiss(window, cx);
                            }
                            true
                        }
                        _ => {
                            this.message = Some("复制失败，请重试。".into());
                            cx.notify();
                            false
                        }
                    }
                })
                .unwrap_or(false);
            if proceed && paste {
                cx.background_executor()
                    .timer(Duration::from_millis(80))
                    .await;
                if let Err(message) = target.paste() {
                    let _ = this.update_in(cx, |this, window, cx| {
                        this.toggle(window, cx);
                        this.message = Some(message);
                        cx.notify();
                    });
                }
            }
        }));
        cx.notify();
    }

    fn action(&mut self, id: String, action: EntryAction, window: &Window, cx: &mut Context<Self>) {
        if self.loading || self.busy {
            return;
        }
        self.busy = true;
        self.message = None;
        let runtime = self.runtime.clone();
        self.action_task = Some(cx.spawn_in(window, async move |this, cx| {
            let response = runtime
                .spawn(
                    async move { backend::action(id, action).await }
                        .instrument(tracing::info_span!("gpui.entry_action")),
                )
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.busy = false;
                match response {
                    Ok(Ok(())) => this.search(window, cx),
                    _ => this.message = Some("操作失败，请重试。".into()),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn paste_paths(&mut self, paths: Vec<String>, window: &mut Window, cx: &mut Context<Self>) {
        let paths = paths
            .into_iter()
            .filter(|p| !p.is_empty())
            .collect::<Vec<_>>();
        if paths.is_empty() {
            self.message = Some("没有可粘贴的文件路径。".into());
            cx.notify();
            return;
        }
        if let Err(message) = self.target.check() {
            self.message = Some(message);
            cx.notify();
            return;
        }
        let text = paths.join("\n");
        let target = self.target.clone();
        self.dismiss(window, cx);
        self.action_task = Some(cx.spawn_in(window, async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(80))
                .await;
            if let Err(message) = target.type_text(&text) {
                let _ = this.update_in(cx, |this, window, cx| {
                    this.toggle(window, cx);
                    this.message = Some(message);
                    cx.notify();
                });
            }
        }));
    }

    fn clear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.filters = Filters::default();
        self.suggestions_open = false;
        self.suggestion_cursor = 0;
        self.input.update(cx, |input, cx| {
            input.set_value("", window, cx);
            input.focus(window, cx);
        });
        self.search(window, cx);
    }

    #[cfg(target_os = "macos")]
    fn clear_action(
        &mut self,
        _: &gpui_component::input::DeleteToBeginningOfLine,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.clear_all(window, cx);
    }

    #[cfg(not(target_os = "macos"))]
    fn clear_action(
        &mut self,
        _: &gpui_component::input::DeleteToPreviousWordStart,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.clear_all(window, cx);
    }

    /// Moves the grid selection one step.
    fn grid_step(
        &mut self,
        direction: crate::grid::Direction,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let next = crate::grid::step(self.selection.index, self.items.len(), direction);
        self.select(next, window, cx);
    }

    /// Left and right move the grid selection while the search box is empty; with text in it
    /// they stay with the caret. The input binds them as actions, so they must be caught here.
    fn grid_sideways(
        &mut self,
        direction: crate::grid::Direction,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let free = self.visible
            && self.filters.images_only()
            && !self.loading
            && self.actions.is_none()
            && self.input.read(cx).value().is_empty();
        if free {
            self.grid_step(direction, window, cx);
            cx.stop_propagation();
        }
    }

    fn move_left_action(
        &mut self,
        _: &gpui_component::input::MoveLeft,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.grid_sideways(crate::grid::Direction::Left, window, cx);
    }

    fn move_right_action(
        &mut self,
        _: &gpui_component::input::MoveRight,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.grid_sideways(crate::grid::Direction::Right, window, cx);
    }

    fn clear_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.visible {
            return;
        }
        self.clear(window, cx);
        cx.stop_propagation();
        cx.notify();
    }

    fn copy_action(
        &mut self,
        _: &gpui_component::input::Copy,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let editing = self.input.update(cx, |input, cx| {
            input.marked_text_range(window, cx).is_some()
                || input
                    .selected_text_range(false, window, cx)
                    .is_some_and(|selection| !selection.range.is_empty())
        });
        if editing {
            cx.propagate();
            return;
        }
        self.restore(false, false, false, window, cx);
        cx.stop_propagation();
    }

    /// Keys while the action list is open. Returns whether the list took the key.
    fn action_list_key(
        &mut self,
        key: &str,
        modifiers: &gpui::Modifiers,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(page) = self.actions.as_ref().map(|list| list.page) else {
            return false;
        };
        let back = |this: &mut Self, window: &mut Window, cx: &mut Context<Self>| {
            if page == ActionsPage::Devices {
                this.actions = this.actions.take().map(|list| ActionList {
                    page: ActionsPage::Main,
                    cursor: 0,
                    ..list
                });
                if let Some((_, rows)) = this.action_rows() {
                    if let Some(list) = this.actions.as_mut() {
                        list.cursor = crate::actions::first_enabled(&rows);
                    }
                }
            } else {
                this.close_actions(window, cx);
            }
        };
        match key {
            "up" | "down" => self.move_action_cursor(key == "down", cx),
            "n" | "p" if modifiers.control => self.move_action_cursor(key == "n", cx),
            "escape" => back(self, window, cx),
            "left" | "backspace" if page == ActionsPage::Devices && !modifiers.platform => {
                back(self, window, cx)
            }
            // Shift and Command with Enter are the direct paste shortcuts; they close the list
            // and fall through.
            "enter" if !modifiers.shift && !modifiers.platform && !modifiers.control => {
                let chosen = self
                    .actions_view()
                    .and_then(|view| view.rows.get(view.cursor).cloned());
                if let Some(row) = chosen.filter(|row| row.enabled) {
                    self.run_action(row.action, window, cx);
                }
            }
            "enter" => {
                self.close_actions(window, cx);
                return false;
            }
            _ => return false,
        }
        true
    }

    fn key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !self.visible {
            return;
        }
        if self.input.update(cx, |input, cx| {
            input.marked_text_range(window, cx).is_some()
        }) {
            return;
        }
        let key = event.keystroke.key.as_str();
        let modifiers = event.keystroke.modifiers;
        let value = self.input.read(cx).value();
        let command = modifiers.platform || modifiers.control;
        if key == "k" && command && !modifiers.shift {
            if self.actions.is_some() {
                self.close_actions(window, cx);
            } else {
                self.open_actions(window, cx);
            }
            cx.stop_propagation();
            cx.notify();
            return;
        }
        if self.actions.is_some() && self.action_list_key(key, &modifiers, window, cx) {
            cx.stop_propagation();
            cx.notify();
            return;
        }
        if key == "escape" {
            if !self.suggestion_options(cx).is_empty() {
                self.suggestions_open = false;
            } else if !value.is_empty() || !self.filters.chips().is_empty() {
                self.clear(window, cx);
            } else {
                self.dismiss(window, cx);
            }
            cx.stop_propagation();
            cx.notify();
            return;
        }
        if self.locked && key == "enter" {
            self.ask_host(HelperRequest::ShowMainWindow, window, cx);
            cx.stop_propagation();
            return;
        }
        if command && key == "o" && modifiers.shift {
            self.ask_host(HelperRequest::ShowMainWindow, window, cx);
            cx.stop_propagation();
            return;
        }
        if command && key == "," && !modifiers.shift {
            self.ask_host(HelperRequest::OpenSettings, window, cx);
            cx.stop_propagation();
            return;
        }
        if key == "backspace" && value.is_empty() && !modifiers.alt {
            if let Some((dimension, value)) = self.filters.chips().last().cloned() {
                self.remove_filter(dimension, &value, window, cx);
                cx.stop_propagation();
                return;
            }
        }
        if key == "l" && command && !modifiers.shift && self.disconnected.is_some() {
            match uc_app_paths::app_log_dir() {
                Some(dir) => {
                    if let Err(message) = platform::open_target(&dir.to_string_lossy()) {
                        self.message = Some(message);
                    }
                }
                None => self.message = Some(strings::NO_LOG_DIR.into()),
            }
            cx.stop_propagation();
            cx.notify();
            return;
        }
        if !self.loading && !self.busy && self.items.is_empty() && !command && !modifiers.shift {
            let shown = self.visible_relaxations().len();
            match key {
                "enter" if self.disconnected.is_some() => self.search(window, cx),
                "enter" if shown > 0 => self.apply_relaxation(window, cx),
                "up" | "down" if shown > 0 => {
                    self.relax_cursor = if key == "down" {
                        (self.relax_cursor + 1) % shown
                    } else {
                        (self.relax_cursor + shown - 1) % shown
                    };
                }
                _ => {}
            }
            if matches!(key, "enter" | "up" | "down") {
                cx.stop_propagation();
                cx.notify();
                return;
            }
        }
        if self.loading || self.busy {
            if matches!(key, "enter" | "up" | "down") {
                cx.stop_propagation();
            }
            return;
        }
        let grid = self.filters.images_only();
        match key {
            "up" | "down" if grid => self.grid_step(
                if key == "up" {
                    crate::grid::Direction::Up
                } else {
                    crate::grid::Direction::Down
                },
                window,
                cx,
            ),
            "up" | "down" => {
                self.selection.move_by(if key == "up" { -1 } else { 1 });
                self.select(self.selection.index, window, cx);
            }
            "n" | "p" if modifiers.control => {
                self.selection.move_by(if key == "p" { -1 } else { 1 });
                self.select(self.selection.index, window, cx);
            }
            // Enter pastes, Shift+Enter pastes plain text, Command+Enter pastes and keeps the panel.
            "enter" => self.restore(true, modifiers.shift && !command, command, window, cx),
            "v" if command && value.is_empty() => {
                self.restore(true, modifiers.shift, false, window, cx)
            }
            "o" if command && !modifiers.shift => self.open_selected(window, cx),
            // Deleting an entry is destructive, so it needs Command/Ctrl+Shift+Backspace.
            // Plain Option+Backspace must stay with the search input (delete previous word).
            "backspace" if (modifiers.platform || modifiers.control) && modifiers.shift => {
                if let Some(item) = self.active_index().and_then(|i| self.items.get(i)) {
                    self.action(item.entry_id.clone(), EntryAction::Delete, window, cx);
                }
            }
            digit
                if (modifiers.platform || modifiers.control)
                    && digit.len() == 1
                    && digit.as_bytes()[0].is_ascii_digit() =>
            {
                // Command+1 to Command+9 paste the rows, or the grid cells, that carry those digits.
                let number = key.parse::<usize>().unwrap_or(0);
                let ix = if grid {
                    crate::grid::entry_for_number(number, self.grid_top, self.items.len())
                } else {
                    let ix = number.wrapping_sub(1);
                    (ix < history::VISIBLE_ROWS && ix < self.items.len()).then_some(ix)
                };
                if let Some(ix) = ix {
                    self.select(ix, window, cx);
                    self.restore(true, modifiers.shift, false, window, cx);
                }
            }
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }
}
