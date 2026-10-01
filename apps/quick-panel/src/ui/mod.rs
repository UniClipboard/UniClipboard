//! The panel as GPUI draws it.
//!
//! [`Panel`] is a thin shell around [`PanelState`]: it turns what happens in the window into
//! calls on the state, and carries out the effects the state asks for (see `effects`). It holds
//! only what belongs to the toolkit: the search box, scroll handles, running tasks, window handles
//! and cached images. Every decision is in the core crate.

pub mod appearance;
mod effects;
mod history;
mod image_preview;
mod images;
mod intents;
mod keyboard;
mod preview_window;
mod view;
mod windows;

gpui::actions!(quick_panel, [NextSuggestion, PreviousSuggestion]);

use crate::platform;
use gpui::{
    prelude::*, App, Context, Entity, EntityInputHandler, KeyDownEvent, RenderImage, ScrollHandle,
    Subscription, Task, Window,
};
use gpui_component::input::{InputEvent, InputState};
use quick_panel_core::ports::{HistoryService, HostLink, PasteTarget};
use quick_panel_core::query::filters::{self, Dimension};
use quick_panel_core::state::{
    ActionsView, Ctx, Effect, Effects, Event, KeyResult, Modifiers, PanelState,
};
use quick_panel_core::text;
use std::{collections::HashMap, rc::Rc, sync::Arc, time::Instant};
use uc_daemon_contract::api::dto::search::SearchResultDto;

/// A decoded bitmap, and the size of the image it was made from.
///
/// The panel owns the bitmap and gives it back to GPUI with `App::drop_image` (see `images`).
#[derive(Clone)]
struct ImageData {
    image: Arc<RenderImage>,
    width: u32,
    height: u32,
    size_bytes: i64,
}

/// What the preview window draws, copied out of the panel.
#[derive(Clone)]
struct PreviewSnapshot {
    item: Option<SearchResultDto>,
    text: Option<String>,
    image: Option<ImageData>,
    loading: bool,
    anchor: Option<quick_panel_core::geometry::window_pair::PreviewAnchor>,
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

/// Work that is running or waiting. Each kind replaces the one before it, and dropping a task
/// cancels it.
#[derive(Default)]
struct Tasks {
    search: Option<Task<()>>,
    action: Option<Task<()>>,
    paste: Option<Task<()>>,
    options: Option<Task<()>>,
    thumbnails: Option<Task<()>>,
    live: Option<Task<()>>,
    preview_timer: Option<Task<()>>,
    preview_load: Option<Task<()>>,
    preview_decode: Option<Task<()>>,
    reconnect: Option<Task<()>>,
    count: Option<Task<()>>,
    blur: Option<Task<()>>,
}

pub struct Panel {
    state: PanelState,
    input: Entity<InputState>,
    scroll: ScrollHandle,
    runtime: tokio::runtime::Handle,
    history: Arc<dyn HistoryService>,
    host: Arc<dyn HostLink>,
    target: Rc<dyn PasteTarget>,
    tasks: Tasks,
    preview_window: Option<gpui::AnyWindowHandle>,
    preview_anchor: Option<quick_panel_core::geometry::window_pair::PreviewAnchor>,
    image_bounds: HashMap<String, gpui::Bounds<gpui::Pixels>>,
    /// Small bitmaps for the entries in the list, by entry id.
    thumbnails: HashMap<String, ImageData>,
    /// The full-size bitmap of the entry being previewed. Never more than one.
    preview_image: Option<(String, ImageData)>,
    /// The entry whose preview image has arrived from the daemon and is being decoded.
    preview_decoding: Option<String>,
    anchor: (f64, f64),
    scale: f64,
    /// General settings from the last successful read. They are applied before every show, so the
    /// first frame already has the configured theme instead of waiting for a new request.
    general: Option<uc_daemon_contract::api::dto::settings::GeneralSettingsDto>,
    _subscriptions: Vec<Subscription>,
}

/// What a call into the state needs, owned so that the state can be borrowed mutably meanwhile.
struct CtxData {
    now: Instant,
    target: Rc<dyn PasteTarget>,
    input: String,
    composing: bool,
    selecting: bool,
    language: Option<String>,
}

impl CtxData {
    fn ctx(&self) -> Ctx<'_> {
        Ctx {
            now: self.now,
            target: &*self.target,
            capabilities: platform::CAPABILITIES,
            input: &self.input,
            composing: self.composing,
            selecting: self.selecting,
            today: chrono::Local::now().date_naive(),
            configured_language: self.language.as_deref(),
            system_language: platform::system_language,
        }
    }
}

impl Panel {
    pub fn new(
        runtime: tokio::runtime::Handle,
        history: Arc<dyn HistoryService>,
        host: Arc<dyn HostLink>,
        target: Rc<dyn PasteTarget>,
        message: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(text::SEARCH_PLACEHOLDER));
        input.update(cx, |input, cx| input.focus(window, cx));
        let subscription = cx.subscribe_in(&input, window, |this, _, event, window, cx| {
            if let InputEvent::Change = event {
                this.input_changed(window, cx);
            }
        });
        let activation = cx.observe_window_activation(window, |this, window, cx| {
            let event = if window.is_window_active() {
                Event::Activated
            } else {
                Event::Deactivated
            };
            this.deliver(event, window, cx);
        });
        let bounds = window.bounds();
        let mut state = PanelState::new(Instant::now());
        state.session.message = message;
        let mut panel = Self {
            state,
            input,
            scroll: ScrollHandle::new(),
            runtime,
            history,
            host,
            target,
            tasks: Tasks::default(),
            preview_window: None,
            preview_anchor: None,
            image_bounds: HashMap::new(),
            thumbnails: HashMap::new(),
            preview_image: None,
            preview_decoding: None,
            anchor: (f64::from(bounds.origin.x), f64::from(bounds.origin.y)),
            scale: std::env::var("UC_GPUI_SCALE")
                .ok()
                .and_then(|s| s.parse::<f64>().ok())
                .filter(|s| s.is_finite())
                .unwrap_or(1.)
                .clamp(0.8, 1.5),
            general: None,
            _subscriptions: vec![subscription, activation],
        };
        let effects = panel.state.start();
        panel.run(effects, window, cx);
        panel.watch(window, cx);
        panel
    }

    /// The search box, the target and the clock as the state sees them.
    fn ctx_data(&self, cx: &App) -> CtxData {
        CtxData {
            now: Instant::now(),
            target: self.target.clone(),
            input: self.input.read(cx).value().to_string(),
            composing: false,
            selecting: false,
            language: self
                .general
                .as_ref()
                .and_then(|general| general.language.clone()),
        }
    }

    /// Like [`Panel::ctx_data`], and asks the search box whether text is being composed or
    /// selected, which decides whether a key belongs to the box or to the panel.
    fn ctx_data_editing(&self, window: &mut Window, cx: &mut Context<Self>) -> CtxData {
        let mut data = self.ctx_data(cx);
        (data.composing, data.selecting) = self.input.update(cx, |input, cx| {
            let composing = input.marked_text_range(window, cx).is_some();
            let selecting = input
                .selected_text_range(false, window, cx)
                .is_some_and(|selection| !selection.range.is_empty());
            (composing, selecting)
        });
        data
    }

    /// Reports an event to the state and carries out what follows.
    fn deliver(&mut self, event: Event, window: &mut Window, cx: &mut Context<Self>) {
        let effects = self.feed(event, window, cx);
        self.run(effects, window, cx);
    }

    /// Reports an event to the state and returns what follows, for the caller to carry out.
    fn feed(&mut self, event: Event, _: &mut Window, cx: &mut Context<Self>) -> Effects {
        let data = self.ctx_data(cx);
        self.state.on_event(event, &data.ctx())
    }

    /// Calls the state and carries out the effects it returns.
    fn apply(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        call: impl FnOnce(&mut PanelState, &Ctx) -> Effects,
    ) {
        let data = self.ctx_data(cx);
        let effects = call(&mut self.state, &data.ctx());
        self.run(effects, window, cx);
    }

    /// Suggestions for the words typed so far.
    fn suggestion_options(&self, cx: &App) -> Vec<filters::Suggestion> {
        let data = self.ctx_data(cx);
        self.state.suggestion_options(&data.ctx())
    }

    /// Number of leading children the suggestion block adds to the result list.
    fn list_lead(&self, cx: &App) -> usize {
        let data = self.ctx_data(cx);
        self.state.list_lead(&data.ctx())
    }
}
