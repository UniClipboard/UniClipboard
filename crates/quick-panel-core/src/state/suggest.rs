//! Suggestions for the words typed in the search box.

use crate::language::Language;
use crate::query::filters::{self, Suggestion};

use super::{Ctx, Effect, Effects, PanelState};

impl PanelState {
    /// Suggestions for the words typed so far. They are candidates only: nothing is filtered
    /// until one is accepted.
    pub fn suggestion_options(&self, ctx: &Ctx) -> Vec<Suggestion> {
        if !self.suggest.open {
            return vec![];
        }
        let sources = self
            .catalog
            .members
            .iter()
            .map(|member| (member.peer_id.clone(), member.device_name.clone()))
            .collect::<Vec<_>>();
        let catalog = filters::Catalog {
            tags: &self.catalog.tags,
            sources: &sources,
            today: ctx.today,
            initials: Language::current().reads_pinyin_initials(),
        };
        filters::suggestions(ctx.input, &catalog)
            .into_iter()
            .filter(|option| {
                !self
                    .search
                    .filters
                    .contains(option.dimension, &option.value)
            })
            .collect()
    }

    /// Turns the suggested words into a filter and keeps the other words as the query. Returns
    /// the effects, or `None` when there was nothing to accept.
    pub fn accept_suggestion(&mut self, ctx: &Ctx) -> Option<Effects> {
        if self.session.busy {
            return None;
        }
        let options = self.suggestion_options(ctx);
        let at = if self.suggest.focused {
            self.suggest.cursor.min(options.len().saturating_sub(1))
        } else {
            0
        };
        let option = options.get(at)?;
        self.search.filters.accept(option);
        let remaining = filters::remaining_query(ctx.input, &option.matched);
        self.search.filters.query = remaining.clone();
        self.suggest.cursor = 0;
        self.suggest.focused = false;
        self.search.hovered = None;
        self.search.keyboard = true;
        let mut effects = vec![Effect::SetInput(remaining)];
        effects.extend(self.search());
        Some(effects)
    }

    /// Up and Down between the results and the suggestion list, which is drawn above them:
    /// Up at the first result enters the list from below, Down at its end goes back to the
    /// results. Returns whether the key was taken; otherwise the arrows move through the results.
    pub fn arrow_between_zones(&mut self, down: bool, ctx: &Ctx) -> bool {
        let count = self.suggestion_options(ctx).len();
        if count == 0 {
            self.suggest.focused = false;
            return false;
        }
        if self.suggest.focused {
            let cursor = self.suggest.cursor.min(count - 1);
            if down && cursor + 1 < count {
                self.suggest.cursor = cursor + 1;
            } else if down && !self.search.items.is_empty() {
                self.suggest.focused = false;
                self.suggest.cursor = 0;
            } else if !down {
                self.suggest.cursor = cursor.saturating_sub(1);
            }
            return true;
        }
        let columns = if self.search.filters.images_only() {
            crate::grid::COLUMNS
        } else {
            1
        };
        let at_top = self.search.items.is_empty() || self.search.selection.index < columns;
        if down || !at_top {
            return false;
        }
        self.suggest.focused = true;
        self.suggest.cursor = count - 1;
        true
    }

    /// Tab: accept the suggestion at the cursor; without suggestions, cycle the type filter.
    pub fn tab(&mut self, reverse: bool, ctx: &Ctx) -> Effects {
        if ctx.composing {
            return vec![];
        }
        if !reverse {
            if let Some(effects) = self.accept_suggestion(ctx) {
                return effects;
            }
        }
        self.cycle_type(reverse)
    }

    /// The suggestion list has the arrow keys only while there is something in it.
    pub fn normalize_suggestion_focus(&mut self, ctx: &Ctx) {
        self.suggest.focused = self.suggest.focused && !self.suggestion_options(ctx).is_empty();
    }

    /// Number of leading children the suggestion block adds to the result list, so that result
    /// `ix` is child `ix + list_lead`.
    pub fn list_lead(&self, ctx: &Ctx) -> usize {
        usize::from(!self.suggestion_options(ctx).is_empty())
    }
}
