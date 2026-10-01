//! Carrying out what the state asks for.
//!
//! Effects that wait (a search, a preview, a paste) run as tasks on GPUI's executor, with the
//! daemon calls handed to the Tokio runtime. When one finishes it reports back as an
//! [`Event`], and the state answers with the next effects.

use super::*;
use quick_panel_core::ports::{SearchFailure, ServiceError};
use quick_panel_core::state::timing;
use std::collections::VecDeque;
use tracing::Instrument;

impl Panel {
    /// Carries out `effects` in order, and the effects that follow from them.
    pub(super) fn run(&mut self, effects: Effects, window: &mut Window, cx: &mut Context<Self>) {
        let mut queue: VecDeque<Effect> = effects.into();
        while let Some(effect) = queue.pop_front() {
            for follow in self.perform(effect, window, cx).into_iter().rev() {
                queue.push_front(follow);
            }
        }
        self.release_stale_preview_image();
        cx.notify();
    }

    /// Gives back every bitmap and stops the work that would make more.
    fn clear_images(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.tasks.thumbnails = None;
        self.tasks.preview_decode = None;
        self.preview_decoding = None;
        for (_, data) in self.thumbnails.drain() {
            cx.drop_image(data.image, Some(window));
        }
        // The preview window draws this bitmap, so it gives the texture back itself (see
        // `ImagePreview::update_source`).
        self.preview_image = None;
    }

    /// Lets go of the preview bitmap once the preview is no longer of its entry.
    fn release_stale_preview_image(&mut self) {
        let wanted = self.state.preview.entry.as_deref();
        if self
            .preview_image
            .as_ref()
            .is_some_and(|(id, _)| Some(id.as_str()) != wanted)
        {
            self.preview_image = None;
        }
    }

    /// Carries out one effect. What it reports at once comes back as effects to run next.
    fn perform(&mut self, effect: Effect, window: &mut Window, cx: &mut Context<Self>) -> Effects {
        match effect {
            Effect::Reposition => self.position(window, cx),
            Effect::Layout => self.layout(window, cx),
            Effect::CaptureTarget => self.target = platform::capture_paste_target(),
            Effect::ApplyTheme => {
                if crate::ui::appearance::apply(self.general.as_ref(), window, cx).is_err() {
                    tracing::warn!("Could not apply quick panel theme settings");
                }
            }
            Effect::ShowWindow => {
                if let Err(error) = platform::set_visible(window, true) {
                    return self.feed(Event::WindowFailed(error), window, cx);
                }
            }
            Effect::HideWindow => {
                let event = match platform::set_visible(window, false) {
                    Ok(()) => Event::WindowHidden,
                    Err(error) => Event::WindowFailed(error),
                };
                return self.feed(event, window, cx);
            }
            Effect::RaiseWindow => {
                let _ = platform::set_visible(window, true);
            }
            Effect::ReturnFocus => self.target.return_focus(),
            Effect::ShowPreviewWindow => self.show_preview(window, cx),
            Effect::HidePreviewWindow => self.hide_preview(cx),
            Effect::ResetInput => self.set_input("", window, cx),
            Effect::SetInput(value) => self.set_input(&value, window, cx),
            Effect::FocusInput => self.input.update(cx, |input, cx| input.focus(window, cx)),
            Effect::ScrollListToTop => self
                .scroll
                .set_offset(gpui::point(gpui::px(0.), gpui::px(0.))),
            Effect::ScrollToItem(ix) => {
                let lead = self.list_lead(cx);
                self.scroll.scroll_to_item(ix + lead);
            }
            Effect::ClearImages => self.clear_images(window, cx),
            Effect::ClearImageBounds => self.image_bounds.clear(),
            Effect::ClearPreviewAnchor => self.preview_anchor = None,
            Effect::Search {
                revision,
                filters,
                debounce,
            } => self.start_search(revision, filters, debounce, window, cx),
            Effect::CancelSearch => self.tasks.search = None,
            Effect::CountRelaxations { revision, queries } => {
                self.count_relaxations(revision, queries, window, cx);
            }
            Effect::ScheduleReconnect(delay) => {
                self.tasks.reconnect = Some(cx.spawn_in(window, async move |this, cx| {
                    cx.background_executor().timer(delay).await;
                    let _ = this.update_in(cx, |this, window, cx| {
                        this.deliver(Event::ReconnectDue, window, cx);
                    });
                }));
            }
            Effect::CancelReconnect => self.tasks.reconnect = None,
            Effect::LoadOptions => self.load_options(window, cx),
            Effect::LoadThumbnails => self.load_thumbnails(window, cx),
            Effect::SchedulePreview { id, kind, delay } => {
                self.tasks.preview_timer = Some(cx.spawn_in(window, async move |this, cx| {
                    cx.background_executor().timer(delay).await;
                    let _ = this.update_in(cx, |this, window, cx| {
                        this.deliver(Event::PreviewDue { id, kind }, window, cx);
                    });
                }));
            }
            Effect::LoadPreview { id, kind } => {
                let runtime = self.runtime.clone();
                let history = self.history.clone();
                self.tasks.preview_load = Some(cx.spawn_in(window, async move |this, cx| {
                    let target = id.clone();
                    let result = runtime
                        .spawn(
                            async move { history.preview(target, kind).await }
                                .instrument(tracing::info_span!("gpui.preview")),
                        )
                        .await
                        .unwrap_or(Err(ServiceError::PreviewUnreadable));
                    let _ = this.update_in(cx, |this, window, cx| {
                        this.deliver(Event::PreviewLoaded { id, result }, window, cx);
                    });
                }));
            }
            Effect::CancelPreview => {
                self.tasks.preview_timer = None;
                self.tasks.preview_load = None;
            }
            Effect::StoreImage { id, payload, size } => {
                let renderer = cx.svg_renderer();
                let runtime = self.runtime.clone();
                let gate = self.decode_gate.clone();
                self.preview_decoding = Some(id.clone());
                self.tasks.preview_decode = Some(cx.spawn_in(window, async move |this, cx| {
                    // Waiting for the gate happens here, so dropping this task also drops the wait.
                    let Ok(permit) = gate.acquire_owned().await else {
                        return;
                    };
                    let decoded = runtime
                        .spawn_blocking(move || {
                            let _permit = permit;
                            images::full(payload, size, |image| image.to_image_data(renderer).ok())
                        })
                        .await
                        .ok()
                        .flatten();
                    let _ = this.update_in(cx, |this, _, cx| {
                        this.preview_decoding = None;
                        // The preview may have moved on while the image was being decoded; a
                        // bitmap that was never drawn needs no release.
                        if let Some(data) = decoded {
                            if this.state.preview.entry.as_deref() == Some(id.as_str()) {
                                this.preview_image = Some((id, data));
                            }
                        }
                        cx.notify();
                    });
                }));
            }
            Effect::ScheduleBlurCheck(delay) => {
                self.tasks.blur = Some(cx.spawn_in(window, async move |this, cx| {
                    cx.background_executor().timer(delay).await;
                    let _ = this.update_in(cx, |this, window, cx| {
                        this.deliver_blur_check(window, cx);
                    });
                }));
            }
            Effect::CancelBlurCheck => self.tasks.blur = None,
            Effect::Restore {
                id,
                plain,
                paste,
                keep_open,
            } => {
                let runtime = self.runtime.clone();
                let history = self.history.clone();
                self.tasks.action = Some(cx.spawn_in(window, async move |this, cx| {
                    let result = runtime
                        .spawn(
                            async move { history.restore(id, plain).await }
                                .instrument(tracing::info_span!("gpui.restore")),
                        )
                        .await
                        .unwrap_or(Err(ServiceError::RestoreFailed));
                    let _ = this.update_in(cx, |this, window, cx| {
                        this.deliver(
                            Event::Restored {
                                result,
                                paste,
                                keep_open,
                            },
                            window,
                            cx,
                        );
                    });
                }));
            }
            Effect::PasteToTarget { keep_open, delay } => {
                let target = self.target.clone();
                self.tasks.paste = Some(cx.spawn_in(window, async move |this, cx| {
                    cx.background_executor().timer(delay).await;
                    let event = match target.paste() {
                        Err(error) => Event::PasteFailed(error),
                        Ok(()) => {
                            if keep_open {
                                cx.background_executor().timer(timing::RAISE_DELAY).await;
                            }
                            Event::PasteDelivered { keep_open }
                        }
                    };
                    let _ = this.update_in(cx, |this, window, cx| this.deliver(event, window, cx));
                }));
            }
            Effect::TypeText { text, delay } => {
                let target = self.target.clone();
                self.tasks.paste = Some(cx.spawn_in(window, async move |this, cx| {
                    cx.background_executor().timer(delay).await;
                    if let Err(error) = target.type_text(&text) {
                        let _ = this.update_in(cx, |this, window, cx| {
                            this.deliver(Event::PasteFailed(error), window, cx);
                        });
                    }
                }));
            }
            Effect::EntryAction { id, action } => {
                let runtime = self.runtime.clone();
                let history = self.history.clone();
                self.tasks.action = Some(cx.spawn_in(window, async move |this, cx| {
                    let result = runtime
                        .spawn(
                            async move { history.action(id, action).await }
                                .instrument(tracing::info_span!("gpui.entry_action")),
                        )
                        .await
                        .unwrap_or(Err(ServiceError::ActionFailed));
                    let _ = this.update_in(cx, |this, window, cx| {
                        this.deliver(Event::ActionDone(result), window, cx);
                    });
                }));
            }
            Effect::OpenTarget(target) => {
                return self.feed(Event::Opened(platform::open_target(&target)), window, cx);
            }
            Effect::RevealPath(path) => {
                return self.feed(Event::Revealed(platform::reveal_path(&path)), window, cx);
            }
            Effect::OpenLogs => match uc_app_paths::app_log_dir() {
                Some(dir) => {
                    if let Err(error) = platform::open_target(&dir.to_string_lossy()) {
                        self.state.set_message(error.to_string());
                    }
                }
                None => self.state.set_message(text::NO_LOG_DIR),
            },
            Effect::HostRequest(request) => {
                return self.feed(Event::HostRequested(self.host.send(request)), window, cx);
            }
            Effect::ApplySettings(settings) => {
                self.apply_settings((*settings).as_ref(), window, cx)
            }
        }
        vec![]
    }

    fn set_input(&mut self, value: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |input, cx| {
            input.set_value(value.to_string(), window, cx);
            input.focus(window, cx);
        });
    }

    /// Reports the content of the search box to the state after an edit.
    pub(super) fn input_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let value = self.input.read(cx).value().to_string();
        self.deliver(Event::InputChanged(value), window, cx);
    }

    /// Whether either window still has the focus, asked when the panel window lost it.
    pub(super) fn dismiss_if_unfocused(&mut self, history: &mut Window, cx: &mut Context<Self>) {
        let panel_active = history.is_window_active();
        let preview_active = self.preview_active(cx);
        self.deliver(
            Event::BlurChecked {
                panel_active,
                preview_active,
            },
            history,
            cx,
        );
    }

    fn deliver_blur_check(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.dismiss_if_unfocused(window, cx);
    }

    fn preview_active(&self, cx: &mut Context<Self>) -> bool {
        self.preview_window
            .and_then(|handle| {
                handle
                    .update(cx, |_, window, _| window.is_window_active())
                    .ok()
            })
            .unwrap_or(false)
    }

    fn start_search(
        &mut self,
        revision: u64,
        filters: filters::Filters,
        debounce: std::time::Duration,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let runtime = self.runtime.clone();
        let history = self.history.clone();
        self.tasks.search = Some(cx.spawn_in(window, async move |this, cx| {
            if !debounce.is_zero() {
                cx.background_executor().timer(debounce).await;
            }
            let (mut send, receive) = tokio::sync::oneshot::channel();
            runtime.spawn(
                async move {
                    tracing::debug!("Quick panel query started");
                    let result = tokio::select! {
                        result = history.search(filters) => result,
                        _ = send.closed() => return,
                    };
                    if result.is_err() {
                        tracing::warn!("Quick panel query failed");
                    }
                    let _ = send.send(result);
                }
                .instrument(tracing::info_span!("gpui.search", request_id = revision)),
            );
            let result = receive.await.unwrap_or(Err(SearchFailure::Interrupted));
            let _ = this.update_in(cx, |this, window, cx| {
                this.deliver(Event::SearchDone { revision, result }, window, cx);
            });
        }));
    }

    fn count_relaxations(
        &mut self,
        revision: u64,
        queries: Vec<filters::Filters>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let runtime = self.runtime.clone();
        let history = self.history.clone();
        self.tasks.count = Some(cx.spawn_in(window, async move |this, cx| {
            let mut totals = vec![];
            for filters in queries {
                let history = history.clone();
                totals.push(
                    runtime
                        .spawn(async move { history.count(filters).await })
                        .await
                        .ok()
                        .flatten(),
                );
            }
            let _ = this.update_in(cx, |this, window, cx| {
                this.deliver(Event::CountsDone { revision, totals }, window, cx);
            });
        }));
    }

    fn load_options(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let runtime = self.runtime.clone();
        let history = self.history.clone();
        self.tasks.options = Some(cx.spawn_in(window, async move |this, cx| {
            let result = runtime
                .spawn(
                    async move { history.options().await }
                        .instrument(tracing::info_span!("gpui.options")),
                )
                .await
                .unwrap_or(Err(ServiceError::TagsUnavailable));
            let _ = this.update_in(cx, |this, window, cx| {
                if result.is_err() {
                    tracing::warn!("Could not load quick panel filter options");
                }
                this.deliver(Event::OptionsLoaded(result.map(Box::new)), window, cx);
            });
        }));
    }

    /// What the settings say about the shortcut, the double-tap trigger and the theme.
    fn apply_settings(
        &mut self,
        settings: Option<&uc_daemon_contract::api::dto::settings::SettingsDto>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(settings) = settings {
            if cx
                .global_mut::<crate::app::hotkey::Shortcuts>()
                .configure(settings)
                .is_err()
            {
                self.state.set_message(text::SHORTCUT_TAKEN);
            }
            self.apply_double_tap_modifier(settings.quick_panel.double_tap_modifier, cx);
            self.general = Some(settings.general.clone());
        }
        if crate::ui::appearance::apply(settings.map(|s| &s.general), window, cx).is_err() {
            tracing::warn!("Could not apply quick panel theme settings");
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
        let monitor = cx.global::<crate::app::double_tap::DoubleTap>().0.clone();
        self.runtime.spawn_blocking(move || {
            if monitor.set_modifier(modifier).is_err() {
                tracing::warn!("Modifier double-tap trigger is unavailable");
            }
        });
    }

    /// Loads the thumbnails of the image entries that have none yet, two at a time.
    ///
    /// A hidden panel draws nothing, so it loads nothing; the next show searches and loads again.
    /// Only one image is decoded at a time, since a screenshot needs width x height x 4 bytes
    /// while it is decoded, and the images in flight are held in memory in their encoded form.
    fn load_thumbnails(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // A new pass replaces the running one, even when nothing is left to load: the old pass
        // must not put back a bitmap that was just given back.
        self.tasks.thumbnails = None;
        if !self.state.session.visible {
            return;
        }
        // Bitmaps of entries that left the list are given back; a session cannot pile them up.
        let listed: std::collections::HashSet<&str> = self
            .state
            .search
            .items
            .iter()
            .map(|item| item.entry_id.as_str())
            .collect();
        let gone: Vec<String> = self
            .thumbnails
            .keys()
            .filter(|id| !listed.contains(id.as_str()))
            .cloned()
            .collect();
        for id in gone {
            if let Some(data) = self.thumbnails.remove(&id) {
                cx.drop_image(data.image, Some(window));
            }
        }
        let ids = self
            .state
            .search
            .items
            .iter()
            .filter(|item| {
                item.content_type == "image" && !self.thumbnails.contains_key(&item.entry_id)
            })
            .map(|item| item.entry_id.clone())
            .collect::<Vec<_>>();
        if ids.is_empty() {
            return;
        }
        let runtime = self.runtime.clone();
        let history = self.history.clone();
        let gate = self.decode_gate.clone();
        self.tasks.thumbnails = Some(cx.spawn_in(window, async move |this, cx| {
            let (send, mut receive) = tokio::sync::mpsc::channel(4);
            runtime.spawn(
                async move {
                    let mut tasks = tokio::task::JoinSet::new();
                    let mut ids = ids.into_iter();
                    loop {
                        while tasks.len() < 2 {
                            let Some(id) = ids.next() else {
                                break;
                            };
                            let history = history.clone();
                            let gate = gate.clone();
                            tasks.spawn(
                                async move {
                                    let result = history.preview(id.clone(), "image".into()).await;
                                    // Decoding is CPU work and must not hold a runtime worker.
                                    let decoded = match result {
                                        Ok(data) => {
                                            let size = data.size;
                                            match (data.image, gate.acquire_owned().await) {
                                                (Some(payload), Ok(permit)) => {
                                                    tokio::task::spawn_blocking(move || {
                                                        let _permit = permit;
                                                        images::thumbnail(payload, size)
                                                    })
                                                    .await
                                                    .ok()
                                                    .flatten()
                                                }
                                                _ => None,
                                            }
                                        }
                                        Err(_) => None,
                                    };
                                    (id, decoded)
                                }
                                .in_current_span(),
                            );
                        }
                        if tasks.is_empty() {
                            break;
                        }
                        tokio::select! {
                            result = tasks.join_next() => {
                                if let Some(Ok(result)) = result {
                                    if send.send(result).await.is_err() {
                                        break;
                                    }
                                }
                            },
                            _ = send.closed() => break,
                        }
                    }
                }
                .instrument(tracing::info_span!("gpui.thumbnails")),
            );
            while let Some((id, decoded)) = receive.recv().await {
                let _ = this.update(cx, |this, cx| {
                    // Only a bitmap that is still wanted is kept: the panel is open and the entry
                    // is in the current results. Anything else was never drawn, so dropping it
                    // is all the release it needs.
                    let wanted = this.state.session.visible
                        && this
                            .state
                            .search
                            .items
                            .iter()
                            .any(|item| item.entry_id == id);
                    if let Some(data) = decoded.filter(|_| wanted) {
                        this.thumbnails.insert(id, data);
                    }
                    cx.notify();
                });
            }
        }));
    }

    /// Follows the daemon's event stream for as long as the panel lives.
    pub(super) fn watch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        use quick_panel_core::ports::Live;
        let runtime = self.runtime.clone();
        let history = self.history.clone();
        self.tasks.live = Some(cx.spawn_in(window, async move |this, cx| loop {
            let (send, mut receive) = tokio::sync::mpsc::channel(8);
            let stream = history.clone();
            runtime.spawn(
                async move { stream.watch(send).await }
                    .instrument(tracing::info_span!("gpui.realtime")),
            );
            while let Some(first) = receive.recv().await {
                let mut event = first;
                if event == Live::Changed {
                    cx.background_executor().timer(timing::LIVE_COALESCE).await;
                    // Several changes are one search, but a lock among them must win.
                    while let Ok(next) = receive.try_recv() {
                        if next != Live::Changed {
                            event = next;
                        }
                    }
                }
                if event == Live::ContentLocked {
                    tracing::warn!("Quick panel dropped its content: content is locked");
                }
                if this
                    .update_in(cx, |this, window, cx| {
                        this.deliver(Event::Live(event), window, cx);
                    })
                    .is_err()
                {
                    return;
                }
            }
            cx.background_executor()
                .timer(timing::LIVE_RESUBSCRIBE_DELAY)
                .await;
        }));
    }
}
