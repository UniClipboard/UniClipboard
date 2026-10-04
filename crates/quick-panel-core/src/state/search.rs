//! Searching, the result list and its selection.

use uc_daemon_contract::api::dto::search::SearchQueryResultDto;
use uc_daemon_contract::api::dto::settings::QuickPanelPositionDto;

use crate::empty_page;
use crate::grid;
use crate::ports::{Choices, Live, Options, SearchFailure, ServiceError};
use crate::query::filters::{Dimension, Filters};

use super::{timing, Ctx, Effect, Effects, PanelState, Preview};

impl PanelState {
    pub fn search(&mut self) -> Effects {
        self.run_search(false)
    }

    /// A search. A silent one keeps the page on screen while it runs, for retries by the panel
    /// itself.
    pub fn run_search(&mut self, silent: bool) -> Effects {
        if self.session.busy {
            return vec![];
        }
        self.search.revision += 1;
        self.search.loading = !silent;
        self.session.message = None;
        self.search.hovered = None;
        let debounce = if self.search.filters.query.trim().is_empty() {
            std::time::Duration::ZERO
        } else {
            timing::SEARCH_DEBOUNCE
        };
        vec![
            Effect::CancelPreview,
            Effect::Search {
                revision: self.search.revision,
                filters: self.search.filters.clone(),
                debounce,
            },
        ]
    }

    pub(super) fn on_search_done(
        &mut self,
        revision: u64,
        result: Result<SearchQueryResultDto, SearchFailure>,
    ) -> Effects {
        if self.search.revision != revision {
            return vec![];
        }
        let mut effects = vec![];
        self.search.loading = false;
        match result {
            Ok(result) => {
                self.search.locked = false;
                self.search.disconnected = None;
                effects.push(Effect::CancelReconnect);
                self.search.total = result.total;
                self.search.items = result.items;
                effects.push(Effect::ClearImageBounds);
                self.search.grid_top = 0;
                self.search.selection.reset(self.search.items.len());
                effects.push(Effect::ScrollListToTop);
                effects.extend(self.schedule_preview());
                effects.push(Effect::LoadThumbnails);
            }
            Err(error) => {
                self.search.locked = matches!(error, SearchFailure::Locked);
                // Locked and disconnected have pages of their own instead of a message.
                let own_page = self.search.locked || matches!(error, SearchFailure::Disconnected);
                self.session.message = (!own_page).then(|| error.to_string());
                if matches!(error, SearchFailure::Disconnected) {
                    self.search.disconnected = Some(self.search.disconnected.unwrap_or(0) + 1);
                    effects.push(Effect::ScheduleReconnect(timing::RECONNECT_DELAY));
                } else {
                    self.search.disconnected = None;
                    // A locked history is unlocked in the main window; look again soon.
                    if self.search.locked {
                        effects.push(Effect::ScheduleReconnect(timing::RECONNECT_DELAY));
                    }
                }
                self.search.items.clear();
                self.search.total = 0;
                self.search.selection.reset(0);
            }
        }
        self.search.relaxations.clear();
        if self.search.items.is_empty()
            && self.session.message.is_none()
            && !self.search.locked
            && self.search.disconnected.is_none()
        {
            effects.extend(self.load_relaxations());
        }
        if self.search.items.is_empty() {
            self.preview = Preview::default();
            effects.push(Effect::Layout);
        }
        effects
    }

    /// Works out how many entries each way of loosening the search would show.
    fn load_relaxations(&mut self) -> Effects {
        let options = empty_page::relaxations(&self.search.filters, |id| self.device_name(id));
        self.search.relax_cursor = 0;
        if options.is_empty() {
            return vec![];
        }
        let queries: Vec<Filters> = options.iter().map(|o| o.filters.clone()).collect();
        self.search.relaxations = options.into_iter().map(|o| (o, None)).collect();
        vec![Effect::CountRelaxations {
            revision: self.search.revision,
            queries,
        }]
    }

    pub(super) fn on_counts(&mut self, revision: u64, totals: Vec<Option<u32>>) {
        if self.search.revision != revision {
            return;
        }
        for ((_, count), total) in self.search.relaxations.iter_mut().zip(totals) {
            *count = total;
        }
    }

    pub(super) fn on_reconnect_due(&mut self) -> Effects {
        if self.session.visible && (self.search.disconnected.is_some() || self.search.locked) {
            self.run_search(true)
        } else {
            vec![]
        }
    }

    pub(super) fn on_live(&mut self, live: Live) -> Effects {
        match live {
            Live::ContentLocked => self.drop_content(),
            Live::Changed | Live::ContentUnlocked => {
                if self.session.visible && !self.session.busy {
                    self.search()
                } else {
                    vec![]
                }
            }
        }
    }

    /// The daemon says content is locked. Everything derived from history goes at once, shown or
    /// not: rows, thumbnails, previews, the action list and the names of tags and devices. A
    /// search that was already running is discarded by bumping the revision.
    fn drop_content(&mut self) -> Effects {
        self.search.revision += 1;
        self.search.loading = false;
        self.search.locked = true;
        self.search.disconnected = None;
        self.menu = None;
        self.search.items.clear();
        self.search.total = 0;
        self.search.selection.reset(0);
        self.search.grid_top = 0;
        self.preview = Preview::default();
        self.search.relaxations.clear();
        self.catalog.tags.clear();
        self.catalog.members.clear();
        self.session.message = None;
        self.search.hovered = None;
        vec![
            Effect::CancelSearch,
            Effect::CancelReconnect,
            Effect::ClearImages,
            Effect::ClearImageBounds,
            Effect::CancelPreview,
            Effect::ClearPreviewAnchor,
            Effect::HidePreviewWindow,
        ]
    }

    pub(super) fn on_options(&mut self, result: Result<Box<Options>, ServiceError>) -> Effects {
        let Ok(options) = result else {
            return vec![];
        };
        if let Some(settings) = &options.settings {
            self.catalog.cursor_anchored = matches!(
                settings.quick_panel.position,
                QuickPanelPositionDto::FollowCursor
            );
        }
        let Options { choices, settings } = *options;
        // A failed read of the choices keeps what was known, and the settings still apply.
        if let Ok(Choices { tags, members }) = choices {
            self.catalog.tags = tags;
            self.catalog.members = members;
        }
        vec![Effect::ApplySettings(Box::new(settings))]
    }

    pub(super) fn input_changed(&mut self, value: String, ctx: &Ctx) -> Effects {
        if self.session.busy {
            return vec![];
        }
        // The input also reports edits that change nothing, such as Backspace in an empty box.
        if value == self.search.filters.query {
            return vec![];
        }
        self.search.filters.query = value.clone();
        let mut effects = self.close_actions();
        self.suggest.open = !value.is_empty();
        self.suggest.cursor = 0;
        // Typing a filter word (#tag, @device, /type) starts in the suggestions; plain words stay
        // in the results.
        let ctx = Ctx {
            input: &value,
            ..*ctx
        };
        self.suggest.focused = crate::query::filters::typing_a_filter(&value)
            && !self.suggestion_options(&ctx).is_empty();
        self.search.hovered = None;
        self.search.keyboard = true;
        effects.extend(self.search());
        effects
    }

    /// Selects a result. Ignored while the list is not settled.
    pub fn select(&mut self, ix: usize) -> Effects {
        if self.search.loading || self.session.busy {
            return vec![];
        }
        self.search.selection.index = ix;
        self.search.hovered = None;
        self.search.keyboard = true;
        let mut effects = vec![];
        if self.search.filters.images_only() {
            self.search.grid_top =
                grid::first_row_for(ix, self.search.grid_top, self.search.items.len());
        } else {
            effects.push(Effect::ScrollToItem(ix));
        }
        effects.extend(self.schedule_preview());
        effects
    }

    /// Moves the selection up or down by one, as the arrow keys do in the list.
    pub(super) fn step_selection(&mut self, delta: isize) -> Effects {
        self.search.selection.move_by(delta);
        self.select(self.search.selection.index)
    }

    /// Moves the grid selection one step.
    pub fn grid_step(&mut self, direction: grid::Direction) -> Effects {
        let next = grid::step(
            self.search.selection.index,
            self.search.items.len(),
            direction,
        );
        self.select(next)
    }

    /// The pointer moved over the list: from now on it drives the selection preview.
    pub fn pointer_moved(&mut self) {
        self.search.pointer_moved = true;
        self.search.keyboard = false;
    }

    /// The pointer entered a result. `while_loading` says whether that counts while a search is
    /// running.
    pub fn hover(&mut self, ix: usize, while_loading: bool) -> Effects {
        let active = !self.search.keyboard
            && self.search.pointer_moved
            && (while_loading || !self.search.loading);
        if !active {
            return vec![];
        }
        self.search.hovered = Some(ix);
        self.schedule_preview()
    }

    /// Scrolls the image grid by whole rows. Returns whether it moved.
    pub fn scroll_grid(&mut self, rows: isize) -> Effects {
        let next = grid::scrolled(self.search.grid_top, rows, self.search.items.len());
        if next == self.search.grid_top {
            return vec![];
        }
        self.search.grid_top = next;
        vec![Effect::ClearImageBounds]
    }

    /// Removes one filter chip.
    pub fn remove_filter(&mut self, dimension: Dimension, value: &str) -> Effects {
        self.search.filters.remove(dimension, value);
        self.suggest.open = false;
        self.search.hovered = None;
        self.search.keyboard = true;
        let mut effects = vec![Effect::FocusInput];
        effects.extend(self.search());
        effects
    }

    /// Empties the search box and all filters.
    pub fn clear(&mut self) -> Effects {
        self.search.filters = Filters::default();
        self.suggest.open = false;
        self.suggest.cursor = 0;
        let mut effects = vec![Effect::ResetInput];
        effects.extend(self.search());
        effects
    }

    /// Applies the way of loosening the search the cursor is on.
    pub(super) fn apply_relaxation(&mut self) -> Effects {
        let Some(relaxation) = self
            .visible_relaxations()
            .get(self.search.relax_cursor)
            .map(|(relaxation, _)| relaxation.clone())
        else {
            return vec![];
        };
        self.search.filters = relaxation.filters;
        self.suggest.open = false;
        self.search.hovered = None;
        self.search.keyboard = true;
        self.search()
    }

    /// Tab with nothing to accept: cycles the type filter.
    pub(super) fn cycle_type(&mut self, reverse: bool) -> Effects {
        if self.session.busy {
            return vec![];
        }
        self.search.filters.cycle_type(reverse);
        self.suggest.open = false;
        self.search.hovered = None;
        self.search.keyboard = true;
        let mut effects = vec![Effect::FocusInput];
        effects.extend(self.search());
        effects
    }
}
