mod filter_bar;
mod image_preview;
mod preview_window;
mod view;

gpui::actions!(quick_panel, [NextSuggestion, PreviousSuggestion]);

use crate::{
    backend::{self, EntryAction},
    filters::{self, Dimension, Filters},
    platform::{self, PasteTarget},
    selection::Selection,
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

#[derive(Clone)]
struct PreviewSnapshot {
    item: Option<SearchResultDto>,
    text: Option<String>,
    image: Option<ImageData>,
    loading: bool,
    anchor: Option<crate::window_pair::PreviewAnchor>,
    scale: f64,
}

impl PreviewSnapshot {
    fn is_image(&self) -> bool {
        self.item
            .as_ref()
            .is_some_and(|item| item.content_type == "image")
    }
}

pub struct Panel {
    input: Entity<InputState>,
    filter_navigation: bool,
    filter_picker_open: bool,
    filter_generation: usize,
    filter_picker_index: usize,
    filter_scroll: ScrollHandle,
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
    filters: Filters,
    suggestions_open: bool,
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
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("搜索剪贴板…"));
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
            filter_navigation: false,
            filter_picker_open: false,
            filter_generation: 0,
            filter_picker_index: 0,
            filter_scroll: ScrollHandle::new(),
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
            filters: Filters::default(),
            suggestions_open: false,
            tags: filters::BUILTIN_TAGS.iter().map(|s| (*s).into()).collect(),
            members: vec![],
            images: HashMap::new(),
            hovered: None,
            keyboard: true,
            pointer_moved: false,
            visible: true,
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
        self.items.clear();
        self.selection.reset(0);
        self.preview = PreviewState::default();
        self.images.clear();
        self.image_bounds.clear();
        self.preview_anchor = None;
        self.hovered = None;
        self.keyboard = true;
        self.pointer_moved = false;
        self.message = None;
        self.suggestions_open = false;
        self.visible = true;
        self.shown_at = Instant::now();
        self.position(window, cx);
        self.input.update(cx, |input, cx| {
            input.set_value("", window, cx);
            input.focus(window, cx);
        });
        self.load_options(window, cx);
        self.search(window, cx);
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
        self.filter_picker_open = false;
        self.filter_navigation = false;
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
            (crate::window_pair::PANEL_WIDTH + crate::window_pair::POINTER_DEPTH) * self.scale;
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
            let mut item = if self.filters.content_type == 3 {
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
        PreviewSnapshot {
            item,
            image,
            text: self.preview.text.clone(),
            loading: self.preview.loading,
            anchor: self.preview_anchor,
            scale: self.scale,
        }
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
            let (send, mut receive) = tokio::sync::mpsc::channel(1);
            runtime.spawn(
                backend::watch_changes(send).instrument(tracing::info_span!("gpui.realtime")),
            );
            while receive.recv().await.is_some() {
                cx.background_executor()
                    .timer(Duration::from_millis(120))
                    .await;
                while receive.try_recv().is_ok() {}
                if this
                    .update_in(cx, |this, window, cx| {
                        if this.visible && !this.busy {
                            this.search(window, cx);
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

    fn input_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.filters.query = self.input.read(cx).value().to_string();
        self.filter_generation = if self.filter_picker_open {
            self.filter_generation + 1
        } else {
            0
        };
        self.filter_picker_open = false;
        self.filter_navigation = false;
        self.suggestions_open = !self.filters.query.is_empty();
        self.filter_picker_index = 0;
        self.filter_scroll
            .set_offset(gpui::point(gpui::px(0.), gpui::px(0.)));
        self.hovered = None;
        self.keyboard = true;
        self.search(window, cx);
    }

    fn search(&mut self, window: &Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.revision += 1;
        let revision = self.revision;
        self.loading = true;
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
                        this.locked = false; this.total = result.total; this.items = result.items;this.image_bounds.clear();
                        this.selection.reset(this.items.len()); this.scroll.set_offset(gpui::point(gpui::px(0.),gpui::px(0.)));
                        this.schedule_preview(window,cx);this.load_images(window,cx);
                    }
                    Err(error) => { this.locked = matches!(error,backend::SearchFailure::Locked); this.message = Some(error.to_string()); this.items.clear(); this.selection.reset(0); }
                }
                if this.items.is_empty() { this.preview = PreviewState::default(); this.layout(window,cx); }
                cx.notify();
            });
        }));
        cx.notify();
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
        self.scroll.scroll_to_item(ix);
        self.schedule_preview(window, cx);
        cx.notify();
    }

    fn restore(&mut self, paste: bool, plain: bool, window: &mut Window, cx: &mut Context<Self>) {
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
                            this.dismiss(window, cx);
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
        self.filter_picker_open = false;
        self.filter_navigation = false;
        self.input.update(cx, |input, cx| {
            input.set_value("", window, cx);
            input.focus(window, cx);
        });
        self.search(window, cx);
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
        if editing || self.filter_navigation || self.filter_picker_open {
            cx.propagate();
            return;
        }
        self.restore(false, false, window, cx);
        cx.stop_propagation();
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
        if key == "k" && modifiers.platform {
            if self.filter_picker_open {
                self.close_filter_picker(window, cx);
            } else {
                self.open_filter_picker(window, cx);
            }
            cx.stop_propagation();
            return;
        }
        let value = self.input.read(cx).value();
        if key == "escape" {
            if self.suggestions_open || self.filter_picker_open {
                self.close_filter_picker(window, cx);
            } else if !value.is_empty() || !self.filters.chips().is_empty() {
                self.clear(window, cx);
            } else {
                self.dismiss(window, cx);
            }
            cx.stop_propagation();
            cx.notify();
            return;
        }
        if key == "q" && modifiers.platform {
            cx.quit();
            cx.stop_propagation();
            return;
        }
        if self.locked && key == "enter" {
            self.action(String::new(), EntryAction::Unlock, window, cx);
            cx.stop_propagation();
            return;
        }
        if (self.filter_navigation || self.filter_picker_open)
            && modifiers.platform
            && matches!(key, "a" | "c" | "v" | "x")
        {
            return;
        }
        if self.handle_filter_key(event, window, cx) {
            return;
        }
        if key == "backspace" && value.is_empty() && !modifiers.alt {
            if let Some((dimension, value)) = self.filters.chips().last().cloned() {
                self.remove_filter(dimension, &value, window, cx);
                cx.stop_propagation();
                return;
            }
        }
        if self.loading || self.busy {
            if matches!(key, "enter" | "up" | "down") {
                cx.stop_propagation();
            }
            return;
        }
        match key {
            "up" | "down" => {
                self.selection.move_by(if key == "up" { -1 } else { 1 });
                self.select(self.selection.index, window, cx);
            }
            "n" | "p" if modifiers.control => {
                self.selection.move_by(if key == "p" { -1 } else { 1 });
                self.select(self.selection.index, window, cx);
            }
            "enter" => self.restore(true, modifiers.alt, window, cx),
            "v" if (modifiers.platform || modifiers.control) && value.is_empty() => {
                self.restore(true, modifiers.alt, window, cx)
            }
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
                let number = key.parse::<usize>().unwrap_or(0);
                let ix = if number == 0 { 9 } else { number - 1 };
                if ix < self.items.len() {
                    self.select(ix, window, cx);
                    self.restore(true, modifiers.alt, window, cx);
                }
            }
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }
}
