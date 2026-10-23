use std::cell::RefCell;
use std::time::{Duration, Instant};

use chrono::NaiveDate;
use uc_daemon_contract::api::dto::search::{SearchQueryResultDto, SearchResultDto};

use super::*;
use crate::ports::{Capabilities, PasteTarget, PlatformError};

/// A paste target whose answers the test decides.
struct FakeTarget {
    check: RefCell<Result<(), PlatformError>>,
}

impl FakeTarget {
    fn ready() -> Self {
        Self {
            check: RefCell::new(Ok(())),
        }
    }

    fn failing(error: PlatformError) -> Self {
        Self {
            check: RefCell::new(Err(error)),
        }
    }
}

impl PasteTarget for FakeTarget {
    fn name(&self) -> Option<String> {
        Some("Editor".into())
    }
    fn check(&self) -> Result<(), PlatformError> {
        self.check.borrow().clone()
    }
    fn return_focus(&self) {}
    fn paste(&self) -> Result<(), PlatformError> {
        Ok(())
    }
    fn type_text(&self, _: &str) -> Result<(), PlatformError> {
        Ok(())
    }
}

const FULL: Capabilities = Capabilities {
    auto_paste: true,
    cursor_anchor: true,
    shaped_preview: true,
    open_and_reveal: true,
};

struct Fixture {
    now: Instant,
    target: FakeTarget,
    input: String,
    /// What is typed on the locked page, which has a box of its own.
    passphrase: String,
    capabilities: Capabilities,
}

impl Fixture {
    fn new() -> Self {
        Self {
            now: Instant::now(),
            target: FakeTarget::ready(),
            input: String::new(),
            passphrase: String::new(),
            capabilities: FULL,
        }
    }

    fn ctx(&self) -> Ctx<'_> {
        Ctx {
            now: self.now,
            target: &self.target,
            capabilities: self.capabilities,
            input: &self.input,
            passphrase: &self.passphrase,
            composing: false,
            selecting: false,
            today: NaiveDate::from_ymd_opt(2026, 9, 30).unwrap(),
        }
    }
}

fn entry(id: &str, content_type: &str) -> SearchResultDto {
    serde_json::from_value(serde_json::json!({
        "entryId": id, "contentType": content_type, "activeTimeMs": 0,
        "tags": [], "textPreview": "text", "charCount": 4, "mimeType": "text/plain",
        "fileExtensions": [], "fileNames": [], "filePaths": [], "linkUrls": [],
        "sourceDevice": null, "payloadState": null
    }))
    .unwrap()
}

fn results(ids: &[&str]) -> SearchQueryResultDto {
    SearchQueryResultDto {
        items: ids.iter().map(|id| entry(id, "text")).collect(),
        total: ids.len() as u32,
        has_more: false,
        state: "ready".into(),
    }
}

fn searched(state: &mut PanelState, ids: &[&str]) {
    let revision = state.search.revision;
    state.on_event(
        Event::SearchDone {
            revision,
            result: Ok(results(ids)),
        },
        &Fixture::new().ctx(),
    );
}

/// An opened panel with results on screen.
fn open_panel(ids: &[&str]) -> (PanelState, Fixture) {
    let fixture = Fixture::new();
    let mut state = PanelState::new(fixture.now);
    state.show(&fixture.ctx());
    searched(&mut state, ids);
    (state, fixture)
}

fn key(state: &mut PanelState, fixture: &Fixture, name: &str, modifiers: Modifiers) -> KeyResult {
    state.on_key(name, modifiers, &fixture.ctx())
}

fn plain() -> Modifiers {
    Modifiers::default()
}

fn command() -> Modifiers {
    Modifiers {
        platform: true,
        ..Modifiers::default()
    }
}

fn has(effects: &[Effect], wanted: impl Fn(&Effect) -> bool) -> bool {
    effects.iter().any(wanted)
}

// --- search ---

#[test]
fn an_answer_to_an_old_search_is_ignored() {
    let (mut state, fixture) = open_panel(&["a"]);
    state.search();
    let stale = state.search.revision - 1;
    let effects = state.on_event(
        Event::SearchDone {
            revision: stale,
            result: Ok(results(&["x", "y"])),
        },
        &fixture.ctx(),
    );
    assert!(effects.is_empty());
    assert_eq!(state.search.items.len(), 1);
    assert!(state.search.loading);
}

#[test]
fn typing_searches_after_the_debounce_and_an_empty_query_searches_at_once() {
    let (mut state, fixture) = open_panel(&["a"]);
    let effects = state.on_event(Event::InputChanged("hello".into()), &fixture.ctx());
    assert!(matches!(
        effects.last(),
        Some(Effect::Search { debounce, .. }) if *debounce == timing::SEARCH_DEBOUNCE
    ));
    let effects = state.search();
    assert!(matches!(
        effects.last(),
        Some(Effect::Search { debounce, .. }) if *debounce == timing::SEARCH_DEBOUNCE
    ));
    state.search.filters.query.clear();
    let effects = state.search();
    assert!(matches!(
        effects.last(),
        Some(Effect::Search { debounce, .. }) if debounce.is_zero()
    ));
}

#[test]
fn an_edit_that_changes_nothing_does_not_search_again() {
    let (mut state, fixture) = open_panel(&["a"]);
    let revision = state.search.revision;
    let effects = state.on_event(Event::InputChanged(String::new()), &fixture.ctx());
    assert!(effects.is_empty());
    assert_eq!(state.search.revision, revision);
}

#[test]
fn a_locked_history_has_its_own_page_and_is_looked_at_again() {
    let (mut state, fixture) = open_panel(&["a"]);
    state.search();
    let revision = state.search.revision;
    let effects = state.on_event(
        Event::SearchDone {
            revision,
            result: Err(SearchFailure::Locked),
        },
        &fixture.ctx(),
    );
    assert!(state.search.locked);
    assert!(state.session.message.is_none());
    assert!(state.search.items.is_empty());
    assert!(has(
        &effects,
        |e| matches!(e, Effect::ScheduleReconnect(d) if *d == timing::RECONNECT_DELAY)
    ));
}

#[test]
fn every_failed_attempt_to_reach_the_daemon_is_counted() {
    let (mut state, fixture) = open_panel(&["a"]);
    for attempt in 1..=3 {
        state.run_search(true);
        let revision = state.search.revision;
        state.on_event(
            Event::SearchDone {
                revision,
                result: Err(SearchFailure::Disconnected),
            },
            &fixture.ctx(),
        );
        assert_eq!(state.search.disconnected, Some(attempt));
        assert!(state.session.message.is_none());
    }
    // An answer ends the streak.
    state.run_search(true);
    searched(&mut state, &["a"]);
    assert_eq!(state.search.disconnected, None);
}

#[test]
fn a_plain_failure_shows_a_message_and_does_not_retry() {
    let (mut state, fixture) = open_panel(&["a"]);
    state.search();
    let revision = state.search.revision;
    let effects = state.on_event(
        Event::SearchDone {
            revision,
            result: Err(SearchFailure::Failed),
        },
        &fixture.ctx(),
    );
    assert_eq!(state.session.message.as_deref(), Some("搜索失败，请重试。"));
    assert!(!has(&effects, |e| matches!(
        e,
        Effect::ScheduleReconnect(_)
    )));
}

#[test]
fn a_reconnect_retries_silently_only_while_the_panel_needs_it() {
    let (mut state, fixture) = open_panel(&["a"]);
    assert!(state
        .on_event(Event::ReconnectDue, &fixture.ctx())
        .is_empty());
    state.search.disconnected = Some(1);
    let effects = state.on_event(Event::ReconnectDue, &fixture.ctx());
    assert!(has(&effects, |e| matches!(e, Effect::Search { .. })));
    assert!(
        !state.search.loading,
        "a silent retry keeps the page on screen"
    );
}

#[test]
fn nothing_found_offers_ways_to_loosen_the_search() {
    let (mut state, fixture) = open_panel(&["a"]);
    state.search.filters.tags = vec!["link".into()];
    state.search();
    let revision = state.search.revision;
    let effects = state.on_event(
        Event::SearchDone {
            revision,
            result: Ok(results(&[])),
        },
        &fixture.ctx(),
    );
    assert_eq!(state.search.relaxations.len(), 1);
    assert!(has(&effects, |e| matches!(
        e,
        Effect::CountRelaxations { .. }
    )));
    // A way that would find nothing is not shown; counts of another search are ignored.
    state.on_event(
        Event::CountsDone {
            revision: revision + 1,
            totals: vec![Some(0)],
        },
        &fixture.ctx(),
    );
    assert_eq!(state.visible_relaxations().len(), 1);
    state.on_event(
        Event::CountsDone {
            revision,
            totals: vec![Some(0)],
        },
        &fixture.ctx(),
    );
    assert!(state.visible_relaxations().is_empty());
}

#[test]
fn a_lock_drops_everything_even_while_hidden() {
    let fixture = Fixture::new();
    let mut state = PanelState::new(fixture.now);
    searched(&mut state, &["a", "b"]);
    state.catalog.tags = vec!["work".into()];
    let revision = state.search.revision;
    let effects = state.on_event(Event::Live(Live::ContentLocked), &fixture.ctx());
    assert!(state.search.locked);
    assert!(state.search.items.is_empty());
    assert!(state.catalog.tags.is_empty());
    assert!(state.catalog.members.is_empty());
    assert!(
        state.search.revision > revision,
        "a running search is discarded"
    );
    for wanted in [
        has(&effects, |e| matches!(e, Effect::CancelSearch)),
        has(&effects, |e| matches!(e, Effect::ClearImages)),
        has(&effects, |e| matches!(e, Effect::HidePreviewWindow)),
    ] {
        assert!(wanted);
    }
}

#[test]
fn a_change_searches_again_only_while_shown_and_idle() {
    let fixture = Fixture::new();
    let mut state = PanelState::new(fixture.now);
    assert!(state
        .on_event(Event::Live(Live::Changed), &fixture.ctx())
        .is_empty());
    state.show(&fixture.ctx());
    assert!(has(
        &state.on_event(Event::Live(Live::Changed), &fixture.ctx()),
        |e| matches!(e, Effect::Search { .. })
    ));
    state.session.busy = true;
    assert!(state
        .on_event(Event::Live(Live::ContentUnlocked), &fixture.ctx())
        .is_empty());
}

// --- opening and closing ---

#[test]
fn showing_starts_afresh_and_keeps_what_the_daemon_told() {
    let (mut state, fixture) = open_panel(&["a", "b"]);
    state.catalog.tags = vec!["work".into()];
    state.search.filters.tags = vec!["work".into()];
    state.session.message = Some("old".into());
    state.session.visible = false;
    let effects = state.show(&fixture.ctx());
    assert!(state.session.visible);
    assert!(state.search.items.is_empty());
    assert!(state.search.filters.chips().is_empty());
    assert!(state.session.message.is_none());
    assert_eq!(state.catalog.tags, ["work"]);
    // The target is captured before anything covers it; the window is shown last.
    assert!(matches!(effects.first(), Some(Effect::CaptureTarget)));
    assert!(matches!(effects.last(), Some(Effect::ShowWindow)));
    assert!(has(&effects, |e| matches!(e, Effect::LoadOptions)));
}

#[test]
fn toggling_an_open_panel_hides_it() {
    let (mut state, fixture) = open_panel(&["a"]);
    let effects = state.toggle(&fixture.ctx());
    assert!(matches!(effects.as_slice(), [Effect::HideWindow]));
    assert!(
        state.session.visible,
        "hidden only once the window reports it"
    );
    let effects = state.on_event(Event::WindowHidden, &fixture.ctx());
    assert!(!state.session.visible);
    assert!(has(&effects, |e| matches!(e, Effect::ReturnFocus)));
    assert!(has(&effects, |e| matches!(e, Effect::CancelSearch)));
    assert!(has(&effects, |e| matches!(e, Effect::HidePreviewWindow)));
}

#[test]
fn hiding_the_panel_gives_back_its_images() {
    let fixture = Fixture::new();
    let mut state = PanelState::new(fixture.now);
    searched(&mut state, &["a", "b"]);
    let effects = state.on_event(Event::WindowHidden, &fixture.ctx());
    assert!(has(&effects, |e| matches!(e, Effect::ClearImages)));
}

#[test]
fn a_window_that_cannot_be_hidden_stays_open_with_the_reason() {
    let (mut state, fixture) = open_panel(&["a"]);
    state.on_event(
        Event::WindowFailed(PlatformError::PanelWindowClosed),
        &fixture.ctx(),
    );
    assert!(state.session.visible);
    assert_eq!(state.session.message.as_deref(), Some("面板窗口已关闭。"));
}

#[test]
fn losing_focus_closes_the_panel_unless_something_holds_it_open() {
    let (mut state, mut fixture) = open_panel(&["a"]);
    // Right after opening the window is still settling.
    assert!(matches!(
        state.on_event(Event::Deactivated, &fixture.ctx()).as_slice(),
        [Effect::ScheduleBlurCheck(delay)] if *delay == timing::BLUR_IGNORE_AFTER_SHOW + timing::BLUR_CHECK_DELAY
    ));
    fixture.now += Duration::from_secs(1);
    let effects = state.on_event(Event::Deactivated, &fixture.ctx());
    assert!(matches!(
        effects.as_slice(),
        [Effect::ScheduleBlurCheck(d)] if *d == timing::BLUR_CHECK_DELAY
    ));
    let blurred = |state: &mut PanelState, fixture: &Fixture, panel, preview| {
        state.on_event(
            Event::BlurChecked {
                panel_active: panel,
                preview_active: preview,
            },
            &fixture.ctx(),
        )
    };
    assert!(blurred(&mut state, &fixture, true, false).is_empty());
    assert!(
        blurred(&mut state, &fixture, false, true).is_empty(),
        "the preview has it"
    );
    state.session.blur_grace_until = Some(fixture.now + Duration::from_millis(500));
    assert!(matches!(
        blurred(&mut state, &fixture, false, false).as_slice(),
        [Effect::ScheduleBlurCheck(delay)] if *delay == Duration::from_millis(500) + timing::BLUR_CHECK_DELAY
    ));
    fixture.now += Duration::from_secs(1);
    assert!(matches!(
        blurred(&mut state, &fixture, false, false).as_slice(),
        [Effect::HideWindow]
    ));
    assert!(matches!(
        state.on_event(Event::Activated, &fixture.ctx()).as_slice(),
        [Effect::CancelBlurCheck]
    ));
}

// --- pasting ---

fn select_first(state: &mut PanelState) {
    state.search.selection.reset(state.search.items.len());
}

#[test]
fn pasting_needs_a_target_that_can_receive_it() {
    let (mut state, mut fixture) = open_panel(&["a"]);
    fixture.target = FakeTarget::failing(PlatformError::NoPastePermission);
    let effects = state.restore(true, false, false, &fixture.ctx());
    assert!(effects.is_empty());
    assert!(!state.session.busy);
    assert_eq!(
        state.session.message.as_deref(),
        Some("需要辅助功能权限才能自动粘贴；也可使用复制按钮。")
    );
    // Copying does not need one.
    state.session.message = None;
    let effects = state.restore(false, false, false, &fixture.ctx());
    assert!(matches!(
        effects.as_slice(),
        [Effect::Restore { paste: false, .. }]
    ));
    assert!(state.session.busy);
}

#[test]
fn a_restored_entry_is_pasted_after_the_panel_hides() {
    let (mut state, fixture) = open_panel(&["a"]);
    let effects = state.restore(true, true, false, &fixture.ctx());
    assert!(matches!(
        effects.as_slice(),
        [Effect::Restore { id, plain: true, paste: true, keep_open: false }] if id == "a"
    ));
    let effects = state.on_event(
        Event::Restored {
            result: Ok(()),
            paste: true,
            keep_open: false,
        },
        &fixture.ctx(),
    );
    assert!(!state.session.busy);
    assert!(matches!(
        effects.as_slice(),
        [Effect::HideWindow, Effect::PasteToTarget { keep_open: false, delay }] if *delay == timing::PASTE_DELAY
    ));
}

#[test]
fn keeping_the_panel_open_holds_off_the_focus_loss_for_a_while() {
    let (mut state, fixture) = open_panel(&["a"]);
    state.restore(true, false, true, &fixture.ctx());
    let effects = state.on_event(
        Event::Restored {
            result: Ok(()),
            paste: true,
            keep_open: true,
        },
        &fixture.ctx(),
    );
    assert!(matches!(
        effects.as_slice(),
        [Effect::PasteToTarget {
            keep_open: true,
            ..
        }]
    ));
    assert_eq!(
        state.session.blur_grace_until,
        Some(fixture.now + timing::BLUR_GRACE)
    );
    let effects = state.on_event(Event::PasteDelivered { keep_open: true }, &fixture.ctx());
    assert!(matches!(effects.as_slice(), [Effect::RaiseWindow]));
    let effects = state.on_event(Event::PasteDelivered { keep_open: false }, &fixture.ctx());
    assert!(effects.is_empty());
}

#[test]
fn a_failed_restore_says_so_and_pastes_nothing() {
    let (mut state, fixture) = open_panel(&["a"]);
    state.restore(true, false, false, &fixture.ctx());
    let effects = state.on_event(
        Event::Restored {
            result: Err(ServiceError::RestoreFailed),
            paste: true,
            keep_open: false,
        },
        &fixture.ctx(),
    );
    assert!(effects.is_empty());
    assert!(!state.session.busy);
    assert_eq!(state.session.message.as_deref(), Some("复制失败，请重试。"));
}

#[test]
fn the_target_is_checked_again_after_the_restore() {
    let (mut state, mut fixture) = open_panel(&["a"]);
    state.restore(true, false, false, &fixture.ctx());
    fixture.target = FakeTarget::failing(PlatformError::FocusMoved);
    let effects = state.on_event(
        Event::Restored {
            result: Ok(()),
            paste: true,
            keep_open: false,
        },
        &fixture.ctx(),
    );
    assert!(effects.is_empty(), "the panel stays put with the reason");
    assert_eq!(
        state.session.message.as_deref(),
        Some("焦点已切换，请回到目标应用重新唤起面板。")
    );
}

#[test]
fn a_failed_paste_brings_the_panel_back_with_the_reason() {
    let (mut state, fixture) = open_panel(&["a"]);
    state.on_event(Event::WindowHidden, &fixture.ctx());
    let effects = state.on_event(
        Event::PasteFailed(PlatformError::CannotCreatePasteEvent),
        &fixture.ctx(),
    );
    assert!(has(&effects, |e| matches!(e, Effect::ShowWindow)));
    assert_eq!(state.session.message.as_deref(), Some("无法创建粘贴事件。"));
}

#[test]
fn a_platform_without_automatic_paste_only_copies() {
    let (mut state, mut fixture) = open_panel(&["a"]);
    fixture.capabilities = Capabilities {
        auto_paste: false,
        ..FULL
    };
    let effects = state.restore(true, false, false, &fixture.ctx());
    assert!(matches!(
        effects.as_slice(),
        [Effect::Restore { paste: false, .. }]
    ));
    state.session.busy = false;
    let result = key(&mut state, &fixture, "k", command());
    assert!(result.consumed);
    let (_, rows) = state.action_rows(&fixture.ctx()).unwrap();
    assert!(rows
        .iter()
        .all(|row| !matches!(row.action, crate::actions::Action::Paste)));
    assert!(rows
        .iter()
        .any(|row| matches!(row.action, crate::actions::Action::Copy)));
}

#[test]
fn file_paths_are_typed_after_the_panel_hides() {
    let (mut state, fixture) = open_panel(&["a"]);
    let effects = state.paste_paths(
        vec!["/a".into(), String::new(), "/b".into()],
        &fixture.ctx(),
    );
    assert!(matches!(
        effects.as_slice(),
        [Effect::HideWindow, Effect::TypeText { text, .. }] if text == "/a\n/b"
    ));
    let effects = state.paste_paths(vec![String::new()], &fixture.ctx());
    assert!(effects.is_empty());
    assert_eq!(
        state.session.message.as_deref(),
        Some("没有可粘贴的文件路径。")
    );
}

#[test]
fn an_action_locks_the_panel_until_it_is_done() {
    let (mut state, fixture) = open_panel(&["a"]);
    let effects = state.entry_action("a".into(), EntryAction::Favorite(true));
    assert!(matches!(effects.as_slice(), [Effect::EntryAction { .. }]));
    assert!(state.session.busy);
    assert!(state
        .entry_action("a".into(), EntryAction::Delete)
        .is_empty());
    let effects = state.on_event(Event::ActionDone(Ok(())), &fixture.ctx());
    assert!(!state.session.busy);
    assert!(
        has(&effects, |e| matches!(e, Effect::Search { .. })),
        "the list is refreshed"
    );
    state.entry_action("a".into(), EntryAction::Delete);
    state.on_event(
        Event::ActionDone(Err(ServiceError::ActionFailed)),
        &fixture.ctx(),
    );
    assert_eq!(state.session.message.as_deref(), Some("操作失败，请重试。"));
}

// --- keys ---

#[test]
fn escape_goes_from_suggestions_to_filters_to_closing() {
    let (mut state, mut fixture) = open_panel(&["a"]);
    state.search.filters.tags = vec!["link".into()];
    let result = key(&mut state, &fixture, "escape", plain());
    assert!(result.consumed);
    assert!(has(&result.effects, |e| matches!(e, Effect::ResetInput)));
    assert!(state.search.filters.chips().is_empty());
    let result = key(&mut state, &fixture, "escape", plain());
    assert!(matches!(result.effects.as_slice(), [Effect::HideWindow]));
    // With text in the box, the first Escape only clears it.
    fixture.input = "abc".into();
    state.search.filters.query = "abc".into();
    let result = key(&mut state, &fixture, "escape", plain());
    assert!(has(&result.effects, |e| matches!(e, Effect::ResetInput)));
}

#[test]
fn keys_are_left_alone_while_hidden_or_composing() {
    let fixture = Fixture::new();
    let mut state = PanelState::new(fixture.now);
    assert!(!key(&mut state, &fixture, "escape", plain()).consumed);
    let (mut state, fixture) = open_panel(&["a"]);
    let composing = Ctx {
        composing: true,
        ..fixture.ctx()
    };
    assert!(!state.on_key("escape", plain(), &composing).consumed);
}

#[test]
fn arrows_move_the_selection_and_stop_at_the_ends() {
    let (mut state, fixture) = open_panel(&["a", "b", "c"]);
    key(&mut state, &fixture, "down", plain());
    key(&mut state, &fixture, "down", plain());
    key(&mut state, &fixture, "down", plain());
    assert_eq!(state.search.selection.selected(), Some(2));
    let result = key(&mut state, &fixture, "up", plain());
    assert_eq!(state.search.selection.selected(), Some(1));
    assert!(has(&result.effects, |e| matches!(
        e,
        Effect::ScrollToItem(1)
    )));
    assert!(has(
        &result.effects,
        |e| matches!(e, Effect::SchedulePreview { id, .. } if id == "b")
    ));
}

#[test]
fn enter_pastes_and_its_modifiers_choose_how() {
    let (mut state, fixture) = open_panel(&["a"]);
    let result = key(&mut state, &fixture, "enter", plain());
    assert!(matches!(
        result.effects.as_slice(),
        [Effect::Restore {
            plain: false,
            keep_open: false,
            paste: true,
            ..
        }]
    ));
    state.session.busy = false;
    let shift = Modifiers {
        shift: true,
        ..plain()
    };
    let result = key(&mut state, &fixture, "enter", shift);
    assert!(matches!(
        result.effects.as_slice(),
        [Effect::Restore { plain: true, .. }]
    ));
    state.session.busy = false;
    let result = key(&mut state, &fixture, "enter", command());
    assert!(matches!(
        result.effects.as_slice(),
        [Effect::Restore {
            keep_open: true,
            ..
        }]
    ));
}

#[test]
fn command_and_a_digit_paste_that_row() {
    let (mut state, fixture) = open_panel(&["a", "b", "c"]);
    let result = key(&mut state, &fixture, "2", command());
    assert!(result.consumed);
    assert!(matches!(
        result.effects.last(),
        Some(Effect::Restore { id, .. }) if id == "b"
    ));
    state.session.busy = false;
    let result = key(&mut state, &fixture, "7", command());
    assert!(result.effects.is_empty(), "there is no seventh row");
    assert!(result.consumed);
}

#[test]
fn a_busy_panel_swallows_navigation_and_lets_other_keys_through() {
    let (mut state, fixture) = open_panel(&["a", "b"]);
    state.session.busy = true;
    for name in ["enter", "up", "down"] {
        let result = key(&mut state, &fixture, name, plain());
        assert!(result.consumed && result.effects.is_empty(), "{name}");
    }
    assert!(!key(&mut state, &fixture, "x", plain()).consumed);
}

#[test]
fn deleting_needs_the_shifted_shortcut() {
    let (mut state, fixture) = open_panel(&["a"]);
    assert!(!key(&mut state, &fixture, "backspace", command()).consumed);
    let both = Modifiers {
        shift: true,
        ..command()
    };
    let result = key(&mut state, &fixture, "backspace", both);
    assert!(matches!(
        result.effects.as_slice(),
        [Effect::EntryAction {
            action: EntryAction::Delete,
            ..
        }]
    ));
}

fn locked_panel() -> (PanelState, Fixture) {
    let (mut state, fixture) = open_panel(&["a"]);
    state.on_event(Event::Live(Live::ContentLocked), &fixture.ctx());
    (state, fixture)
}

fn type_passphrase(state: &mut PanelState, fixture: &mut Fixture, value: &str) -> Effects {
    if !state.session.needs_passphrase {
        state.on_event(Event::UnlockRequested, &fixture.ctx());
        state.on_event(Event::KeyringUnlockDone(Ok(false)), &fixture.ctx());
    }
    fixture.passphrase = value.into();
    state.on_event(Event::PassphraseChanged, &fixture.ctx())
}

fn submit(state: &mut PanelState, fixture: &Fixture) -> KeyResult {
    key(state, fixture, "enter", plain())
}

#[test]
fn enter_on_the_locked_page_sends_the_passphrase_not_the_search_text() {
    let (mut state, mut fixture) = locked_panel();
    fixture.input = "invoice".into();
    type_passphrase(&mut state, &mut fixture, "correct horse");
    let result = submit(&mut state, &fixture);
    assert!(matches!(
        result.effects.as_slice(),
        [Effect::Unlock(secret)] if secret.expose() == "correct horse"
    ));
    assert_eq!(state.session.unlock, UnlockStatus::Checking);
}

#[test]
fn the_locked_page_tries_the_keyring_before_requesting_a_passphrase() {
    let (mut state, mut fixture) = locked_panel();
    fixture.input = "invoice".into();
    assert!(!state.session.needs_passphrase);
    assert!(matches!(
        submit(&mut state, &fixture).effects.as_slice(),
        [Effect::UnlockFromKeyring]
    ));
    assert_eq!(state.session.unlock, UnlockStatus::CheckingKeyring);
    assert!(submit(&mut state, &fixture).effects.is_empty());
    state.on_event(Event::KeyringUnlockDone(Ok(false)), &fixture.ctx());
    assert!(state.session.needs_passphrase);
    assert_eq!(state.session.unlock, UnlockStatus::Idle);
    assert!(submit(&mut state, &fixture).effects.is_empty());
}

#[test]
fn a_keyring_unlock_refreshes_history_without_opening_the_main_window() {
    let (mut state, fixture) = locked_panel();
    state.on_event(Event::UnlockRequested, &fixture.ctx());
    let effects = state.on_event(Event::KeyringUnlockDone(Ok(true)), &fixture.ctx());
    assert_eq!(state.session.unlock, UnlockStatus::Idle);
    assert!(!state.session.needs_passphrase);
    assert!(has(&effects, |e| matches!(e, Effect::Search { .. })));
    assert!(!has(&effects, |e| matches!(e, Effect::HostRequest(_))));
}

#[test]
fn a_keyring_failure_requests_a_passphrase_and_a_new_opening_retries_the_keyring() {
    let (mut state, fixture) = locked_panel();
    state.on_event(Event::UnlockRequested, &fixture.ctx());
    state.on_event(
        Event::KeyringUnlockDone(Err(UnlockFailure::Failed)),
        &fixture.ctx(),
    );
    assert!(state.session.needs_passphrase);
    state.show(&fixture.ctx());
    assert!(!state.session.needs_passphrase);
    let late = state.on_event(Event::KeyringUnlockDone(Ok(false)), &fixture.ctx());
    assert!(late.is_empty());
    assert!(!state.session.needs_passphrase);
}

#[test]
fn the_keyring_authorization_dialog_does_not_close_the_panel() {
    let (mut state, mut fixture) = locked_panel();
    state.on_event(Event::UnlockRequested, &fixture.ctx());
    fixture.now += std::time::Duration::from_secs(5);
    let effects = state.on_blur_checked(false, false, &fixture.ctx());
    assert!(!has(&effects, |e| matches!(e, Effect::HideWindow)));
}

#[test]
fn a_passphrase_being_checked_is_not_sent_twice() {
    let (mut state, mut fixture) = locked_panel();
    type_passphrase(&mut state, &mut fixture, "secret");
    submit(&mut state, &fixture);
    let again = submit(&mut state, &fixture);
    assert!(!has(&again.effects, |e| matches!(e, Effect::Unlock(_))));
}

#[test]
fn the_locked_page_leaves_the_search_alone() {
    let (mut state, mut fixture) = open_panel(&["a"]);
    fixture.input = "invoice".into();
    state.on_event(Event::InputChanged("invoice".into()), &fixture.ctx());
    state.on_event(Event::Live(Live::ContentLocked), &fixture.ctx());
    assert_eq!(state.search.filters.query, "invoice");
    let revision = state.search.revision;
    type_passphrase(&mut state, &mut fixture, "hunter2");
    assert_eq!(state.search.filters.query, "invoice");
    assert_eq!(state.search.revision, revision);
}

#[test]
fn the_keys_of_the_search_page_do_nothing_on_the_locked_page() {
    let (mut state, mut fixture) = locked_panel();
    type_passphrase(&mut state, &mut fixture, "abc");
    for name in ["up", "down", "left", "right", "tab", "backspace", "k"] {
        let modifiers = if name == "k" { command() } else { plain() };
        let result = key(&mut state, &fixture, name, modifiers);
        assert!(!result.consumed, "{name}");
        assert!(result.effects.is_empty(), "{name}");
    }
    assert!(!state.on_copy(&fixture.ctx()).consumed);
    assert!(!state.on_backspace(&fixture.ctx()).consumed);
    assert!(!state.on_clear_all().consumed);
    assert!(state.menu.is_none());
}

#[test]
fn tab_on_the_locked_page_does_not_change_the_search_type() {
    let (mut state, fixture) = locked_panel();
    let before = state.search.filters.types.clone();
    assert!(state.tab(false, &fixture.ctx()).is_empty());
    assert_eq!(state.search.filters.types, before);
}

#[test]
fn escape_clears_a_typed_passphrase_first_and_then_closes() {
    let (mut state, mut fixture) = locked_panel();
    type_passphrase(&mut state, &mut fixture, "abc");
    let first = key(&mut state, &fixture, "escape", plain());
    assert!(matches!(
        first.effects.as_slice(),
        [Effect::ClearPassphrase]
    ));
    fixture.passphrase.clear();
    let second = key(&mut state, &fixture, "escape", plain());
    assert!(matches!(second.effects.as_slice(), [Effect::HideWindow]));
}

#[test]
fn becoming_locked_empties_the_passphrase_box() {
    let (mut state, fixture) = open_panel(&["a"]);
    let effects = state.on_event(Event::Live(Live::ContentLocked), &fixture.ctx());
    assert!(has(&effects, |e| matches!(e, Effect::ClearPassphrase)));
}

#[test]
fn a_wrong_passphrase_stays_locked_and_empties_the_box() {
    let (mut state, mut fixture) = locked_panel();
    type_passphrase(&mut state, &mut fixture, "nope");
    submit(&mut state, &fixture);
    let revision = state.search.revision;
    let effects = state.on_event(
        Event::UnlockDone(Err(UnlockFailure::WrongPassphrase)),
        &fixture.ctx(),
    );
    assert!(state.search.locked);
    assert_eq!(
        state.session.unlock,
        UnlockStatus::Failed(UnlockFailure::WrongPassphrase)
    );
    assert!(has(&effects, |e| matches!(e, Effect::ClearPassphrase)));
    assert_eq!(state.search.revision, revision);
}

#[test]
fn a_failure_that_is_not_the_passphrase_keeps_what_was_typed() {
    let (mut state, mut fixture) = locked_panel();
    type_passphrase(&mut state, &mut fixture, "right but slow");
    submit(&mut state, &fixture);
    let effects = state.on_event(
        Event::UnlockDone(Err(UnlockFailure::Timeout)),
        &fixture.ctx(),
    );
    assert!(!has(&effects, |e| matches!(e, Effect::ClearPassphrase)));
    assert_eq!(
        state.session.unlock,
        UnlockStatus::Failed(UnlockFailure::Timeout)
    );
}

#[test]
fn editing_after_a_failure_clears_it() {
    let (mut state, mut fixture) = locked_panel();
    type_passphrase(&mut state, &mut fixture, "x");
    submit(&mut state, &fixture);
    state.on_event(
        Event::UnlockDone(Err(UnlockFailure::Timeout)),
        &fixture.ctx(),
    );
    assert!(matches!(state.session.unlock, UnlockStatus::Failed(_)));
    type_passphrase(&mut state, &mut fixture, "xy");
    assert_eq!(state.session.unlock, UnlockStatus::Idle);
}

#[test]
fn a_right_passphrase_empties_the_box_and_looks_at_the_history_again() {
    let (mut state, mut fixture) = locked_panel();
    type_passphrase(&mut state, &mut fixture, "right");
    submit(&mut state, &fixture);
    let effects = state.on_event(Event::UnlockDone(Ok(())), &fixture.ctx());
    assert_eq!(state.session.unlock, UnlockStatus::Idle);
    assert!(has(&effects, |e| matches!(e, Effect::ClearPassphrase)));
    assert!(has(&effects, |e| matches!(e, Effect::Search { .. })));
}

#[test]
fn opening_the_panel_again_forgets_an_earlier_failure() {
    let (mut state, mut fixture) = locked_panel();
    type_passphrase(&mut state, &mut fixture, "x");
    submit(&mut state, &fixture);
    state.on_event(
        Event::UnlockDone(Err(UnlockFailure::WrongPassphrase)),
        &fixture.ctx(),
    );
    state.on_event(Event::WindowHidden, &fixture.ctx());
    let effects = state.show(&fixture.ctx());
    assert_eq!(state.session.unlock, UnlockStatus::Idle);
    assert!(has(&effects, |e| matches!(e, Effect::ClearPassphrase)));
}

#[test]
fn an_unlock_answer_from_an_earlier_opening_is_ignored() {
    let (mut state, mut fixture) = locked_panel();
    type_passphrase(&mut state, &mut fixture, "old");
    submit(&mut state, &fixture);
    state.on_event(Event::WindowHidden, &fixture.ctx());
    state.show(&fixture.ctx());
    type_passphrase(&mut state, &mut fixture, "new");
    let late = state.on_event(
        Event::UnlockDone(Err(UnlockFailure::WrongPassphrase)),
        &fixture.ctx(),
    );
    assert!(late.is_empty(), "{late:?}");
    assert_eq!(state.session.unlock, UnlockStatus::Idle);
    let late_ok = state.on_event(Event::UnlockDone(Ok(())), &fixture.ctx());
    assert!(late_ok.is_empty(), "{late_ok:?}");
}

#[test]
fn an_effect_never_prints_the_passphrase() {
    let effect = Effect::Unlock(Secret::new("hunter2"));
    assert!(!format!("{effect:?}").contains("hunter2"));
}

#[test]
fn the_main_window_stays_reachable_from_the_locked_page() {
    let (mut state, fixture) = locked_panel();
    let result = key(
        &mut state,
        &fixture,
        "o",
        Modifiers {
            platform: true,
            shift: true,
            ..Modifiers::default()
        },
    );
    assert!(matches!(
        result.effects.as_slice(),
        [Effect::HostRequest(HostRequest::ShowMainWindow)]
    ));
}

#[test]
fn the_empty_page_moves_between_suggestions_and_applies_one() {
    let (mut state, fixture) = open_panel(&["a"]);
    state.search.filters.tags = vec!["link".into()];
    state.search.filters.types = vec!["image".into()];
    state.search();
    let revision = state.search.revision;
    state.on_event(
        Event::SearchDone {
            revision,
            result: Ok(results(&[])),
        },
        &fixture.ctx(),
    );
    assert_eq!(state.search.relaxations.len(), 2);
    key(&mut state, &fixture, "down", plain());
    assert_eq!(state.search.relax_cursor, 1);
    key(&mut state, &fixture, "down", plain());
    assert_eq!(state.search.relax_cursor, 0, "the cursor wraps");
    key(&mut state, &fixture, "down", plain());
    let result = key(&mut state, &fixture, "enter", plain());
    assert!(has(&result.effects, |e| matches!(e, Effect::Search { .. })));
    assert_eq!(
        state.search.filters.types,
        ["image"],
        "the type stays, the tag goes"
    );
    assert!(state.search.filters.tags.is_empty());
}

#[test]
fn backspace_in_an_empty_box_removes_the_last_chip() {
    let (mut state, fixture) = open_panel(&["a"]);
    state.search.filters.tags = vec!["link".into(), "code".into()];
    let result = state.on_backspace(&fixture.ctx());
    assert!(result.consumed);
    assert_eq!(state.search.filters.tags, ["link"]);
    assert!(has(&result.effects, |e| matches!(e, Effect::Search { .. })));
    let typed = Fixture {
        input: "x".into(),
        ..Fixture::new()
    };
    assert!(!state.on_backspace(&typed.ctx()).consumed);
}

#[test]
fn the_action_list_opens_walks_and_closes_with_the_keyboard() {
    let (mut state, fixture) = open_panel(&["a"]);
    let result = key(&mut state, &fixture, "k", command());
    assert!(has(&result.effects, |e| matches!(
        e,
        Effect::ShowPreviewWindow
    )));
    assert!(state.menu.as_ref().is_some_and(|menu| menu.forced_preview));
    let first = state.actions_view(&fixture.ctx()).unwrap();
    assert_eq!(first.title, "操作");
    key(&mut state, &fixture, "down", plain());
    assert_eq!(state.actions_view(&fixture.ctx()).unwrap().cursor, 1);
    // The list takes the arrows; the selection stays where it was.
    assert_eq!(state.search.selection.selected(), Some(0));
    let result = key(&mut state, &fixture, "escape", plain());
    assert!(state.menu.is_none());
    assert!(has(&result.effects, |e| matches!(
        e,
        Effect::SchedulePreview { .. }
    )));
}

#[test]
fn choosing_a_device_opens_a_second_page_and_escape_returns() {
    let (mut state, fixture) = open_panel(&["a"]);
    key(&mut state, &fixture, "k", command());
    state.run_action(crate::actions::Action::ChooseDevice, &fixture.ctx());
    assert_eq!(state.actions_view(&fixture.ctx()).unwrap().title, "发送到");
    key(&mut state, &fixture, "escape", plain());
    assert_eq!(state.actions_view(&fixture.ctx()).unwrap().title, "操作");
    key(&mut state, &fixture, "escape", plain());
    assert!(state.menu.is_none());
}

#[test]
fn running_a_list_action_closes_the_list_first() {
    let (mut state, fixture) = open_panel(&["a"]);
    key(&mut state, &fixture, "k", command());
    let effects = state.run_action(crate::actions::Action::Favorite(true), &fixture.ctx());
    assert!(state.menu.is_none());
    assert!(matches!(
        effects.last(),
        Some(Effect::EntryAction {
            action: EntryAction::Favorite(true),
            ..
        })
    ));
}

// --- previews ---

#[test]
fn a_preview_follows_the_selection_after_a_pause_that_depends_on_the_window() {
    let (mut state, fixture) = open_panel(&["a", "b"]);
    let delay_of = |effects: &[Effect]| match effects {
        [Effect::SchedulePreview { delay, .. }] => *delay,
        other => panic!("{other:?}"),
    };
    assert_eq!(
        delay_of(&state.schedule_preview()),
        timing::PREVIEW_DELAY_CLOSED
    );
    let effects = state.on_event(
        Event::PreviewDue {
            id: "a".into(),
            kind: "text".into(),
        },
        &fixture.ctx(),
    );
    assert!(matches!(
        effects.as_slice(),
        [Effect::ShowPreviewWindow, Effect::LoadPreview { .. }]
    ));
    assert!(state.preview.loading && state.preview.expanded);
    // The selected entry is showing already; the next one follows quickly.
    assert!(state.schedule_preview().is_empty());
    state.search.selection.index = 1;
    assert_eq!(
        delay_of(&state.schedule_preview()),
        timing::PREVIEW_DELAY_OPEN
    );
}

#[test]
fn a_preview_answer_for_another_entry_is_dropped() {
    let (mut state, fixture) = open_panel(&["a", "b"]);
    state.on_event(
        Event::PreviewDue {
            id: "a".into(),
            kind: "text".into(),
        },
        &fixture.ctx(),
    );
    let data = |text: &str| PreviewData {
        text: Some(text.into()),
        image: None,
        size: 4,
    };
    state.on_event(
        Event::PreviewLoaded {
            id: "b".into(),
            result: Ok(data("other")),
        },
        &fixture.ctx(),
    );
    assert!(state.preview.loading);
    state.on_event(
        Event::PreviewLoaded {
            id: "a".into(),
            result: Ok(data("mine")),
        },
        &fixture.ctx(),
    );
    assert_eq!(state.preview.text.as_deref(), Some("mine"));
    assert!(!state.preview.loading);
    state.on_event(
        Event::PreviewDue {
            id: "b".into(),
            kind: "text".into(),
        },
        &fixture.ctx(),
    );
    state.on_event(
        Event::PreviewLoaded {
            id: "b".into(),
            result: Err(ServiceError::PreviewUnreadable),
        },
        &fixture.ctx(),
    );
    assert_eq!(
        state.preview.text.as_deref(),
        Some("无法读取预览，请重试。")
    );
}

#[test]
fn a_preview_that_falls_due_after_the_panel_closed_is_not_loaded() {
    let fixture = Fixture::new();
    let mut state = PanelState::new(fixture.now);
    let effects = state.on_event(
        Event::PreviewDue {
            id: "a".into(),
            kind: "text".into(),
        },
        &fixture.ctx(),
    );
    assert!(effects.is_empty());
}

// --- suggestions ---

#[test]
fn typing_a_filter_word_focuses_the_suggestions_and_tab_accepts_it() {
    let (mut state, mut fixture) = open_panel(&["a"]);
    state.catalog.tags = vec!["work".into()];
    fixture.input = "#wo".into();
    state.on_event(Event::InputChanged("#wo".into()), &fixture.ctx());
    assert!(state.suggest.open);
    assert!(
        state.suggest.focused,
        "a filter word starts in the suggestions"
    );
    let effects = state.tab(false, &fixture.ctx());
    assert!(matches!(effects.first(), Some(Effect::SetInput(rest)) if rest.is_empty()));
    assert_eq!(state.search.filters.tags, ["work"]);
    assert!(!state.suggest.focused);
}

#[test]
fn tab_without_suggestions_cycles_the_type_filter() {
    let (mut state, fixture) = open_panel(&["a"]);
    let effects = state.tab(false, &fixture.ctx());
    assert_eq!(state.search.filters.types, ["text"]);
    assert!(has(&effects, |e| matches!(e, Effect::Search { .. })));
    let composing = Ctx {
        composing: true,
        ..fixture.ctx()
    };
    assert!(state.tab(false, &composing).is_empty());
    assert_eq!(state.search.filters.types, ["text"]);
}

// --- misc ---

#[test]
fn the_shell_can_run_the_first_search_when_the_process_starts() {
    let fixture = Fixture::new();
    let mut state = PanelState::new(fixture.now);
    let effects = state.start();
    assert!(matches!(effects.first(), Some(Effect::Reposition)));
    assert!(has(&effects, |e| matches!(e, Effect::LoadOptions)));
    assert!(has(&effects, |e| matches!(e, Effect::Search { .. })));
    assert!(!state.session.visible);
}

#[test]
fn hovering_only_counts_once_the_pointer_took_over_from_the_keyboard() {
    let (mut state, _fixture) = open_panel(&["a", "b"]);
    assert!(state.hover(1, false).is_empty());
    state.pointer_moved();
    let effects = state.hover(1, false);
    assert_eq!(state.search.hovered, Some(1));
    assert!(has(
        &effects,
        |e| matches!(e, Effect::SchedulePreview { id, .. } if id == "b")
    ));
    // The keyboard takes over again with the next selection.
    select_first(&mut state);
    state.select(0);
    assert_eq!(state.search.hovered, None);
    assert!(state.search.keyboard);
}

#[test]
fn what_the_daemon_lists_replaces_the_catalog_and_reaches_the_shell() {
    let (mut state, fixture) = open_panel(&["a"]);
    let effects = state.on_event(
        Event::OptionsLoaded(Ok(Box::new(Options {
            choices: Ok(crate::ports::Choices {
                tags: vec!["work".into()],
                members: vec![],
            }),
            settings: None,
        }))),
        &fixture.ctx(),
    );
    assert_eq!(state.catalog.tags, ["work"]);
    assert!(matches!(effects.as_slice(), [Effect::ApplySettings(_)]));
    // A failed read keeps what was known.
    let effects = state.on_event(
        Event::OptionsLoaded(Err(ServiceError::TagsUnavailable)),
        &fixture.ctx(),
    );
    assert!(effects.is_empty());
    assert_eq!(state.catalog.tags, ["work"]);
}

#[test]
fn no_page_is_drawn_until_the_daemon_has_said_whether_content_is_locked() {
    let fixture = Fixture::new();
    let mut state = PanelState::new(fixture.now);
    state.show(&fixture.ctx());
    assert!(state.awaiting_lock_state());
    let revision = state.search.revision;
    state.on_event(
        Event::SearchDone {
            revision,
            result: Err(SearchFailure::Locked),
        },
        &fixture.ctx(),
    );
    assert!(!state.awaiting_lock_state());
}

#[test]
fn an_unlocked_answer_also_settles_the_lock_state() {
    let (mut state, fixture) = open_panel(&["a"]);
    assert!(!state.awaiting_lock_state());
    state.on_event(Event::WindowHidden, &fixture.ctx());
    state.show(&fixture.ctx());
    assert!(state.search.loading);
    assert!(!state.awaiting_lock_state());
}

#[test]
fn a_lock_notice_settles_the_lock_state_before_any_search_answers() {
    let fixture = Fixture::new();
    let mut state = PanelState::new(fixture.now);
    state.show(&fixture.ctx());
    state.on_event(Event::Live(Live::ContentLocked), &fixture.ctx());
    assert!(!state.awaiting_lock_state());
}

#[test]
fn an_unreachable_daemon_says_nothing_about_the_lock() {
    let fixture = Fixture::new();
    let mut state = PanelState::new(fixture.now);
    state.show(&fixture.ctx());
    let revision = state.search.revision;
    state.on_event(
        Event::SearchDone {
            revision,
            result: Err(SearchFailure::Disconnected),
        },
        &fixture.ctx(),
    );
    assert!(!state.search.lock_known);
    assert!(!state.awaiting_lock_state(), "the disconnected page shows");
}

#[test]
fn a_click_outside_during_show_debounce_is_checked_after_it_expires() {
    let (mut state, mut fixture) = open_panel(&["a"]);
    fixture.now += Duration::from_millis(50);
    let effects = state.on_event(Event::Deactivated, &fixture.ctx());
    assert!(
        matches!(effects.as_slice(), [Effect::ScheduleBlurCheck(delay)]
        if *delay == Duration::from_millis(350))
    );
    fixture.now += Duration::from_millis(350);
    assert!(matches!(
        state
            .on_blur_checked(false, false, &fixture.ctx())
            .as_slice(),
        [Effect::HideWindow]
    ));
    assert!(state
        .on_blur_checked(true, false, &fixture.ctx())
        .is_empty());
    assert!(state
        .on_blur_checked(false, true, &fixture.ctx())
        .is_empty());
}

#[test]
fn completing_keyring_authorization_rechecks_focus() {
    for result in [Ok(false), Ok(true), Err(UnlockFailure::Failed)] {
        let (mut pending, fixture) = locked_panel();
        pending.on_event(Event::UnlockRequested, &fixture.ctx());
        let effects = pending.on_event(Event::KeyringUnlockDone(result), &fixture.ctx());
        assert!(has(&effects, |effect| matches!(
            effect,
            Effect::ScheduleBlurCheck(_)
        )));
    }
}
