//! The panel's behavior as a state machine, free of any UI toolkit.
//!
//! [`PanelState`] holds everything the panel decides on. Its methods change the state
//! synchronously and return the [`Effect`]s the shell has to carry out: searching, pasting,
//! showing a window. The shell reports what came of an effect by calling [`PanelState::on_event`].
//! Nothing here waits, reads a clock or touches the operating system, so every rule can be tested
//! by calling methods and looking at the state and the returned effects.

mod keys;
mod menu;
mod preview;
mod search;
mod session;
mod suggest;
#[cfg(test)]
mod tests;

use std::time::{Duration, Instant};

use chrono::NaiveDate;
use uc_daemon_contract::api::dto::search::{SearchQueryResultDto, SearchResultDto};
use uc_daemon_contract::api::dto::settings::SettingsDto;
use uc_daemon_contract::api::types::SpaceMemberDto;

use crate::actions::Row;
use crate::empty_page::Relaxation;
use crate::ports::{
    Capabilities, EntryAction, HostRequest, ImagePayload, Live, Options, PasteTarget,
    PlatformError, PreviewData, SearchFailure, ServiceError,
};
use crate::query::filters::{self, Filters};
use crate::selection::Selection;

pub use keys::{KeyResult, Modifiers, VISIBLE_ROWS};
pub use menu::{ActionMenu, ActionsPage, ActionsView};

/// How long the panel waits before acting. The values are part of how the panel feels and are
/// checked against the real application, so they are named here instead of written inline.
pub mod timing {
    use std::time::Duration;

    /// Typing pauses this long before a search starts, so a word is not searched letter by letter.
    pub const SEARCH_DEBOUNCE: Duration = Duration::from_millis(300);
    /// Several changes announced by the daemon at once become one search.
    pub const LIVE_COALESCE: Duration = Duration::from_millis(120);
    /// A preview follows the selection after this long once the preview is already open ...
    pub const PREVIEW_DELAY_OPEN: Duration = Duration::from_millis(120);
    /// ... and after this long when it is not.
    pub const PREVIEW_DELAY_CLOSED: Duration = Duration::from_millis(500);
    /// The pause between hiding the panel and pasting, so the target has focus by then.
    pub const PASTE_DELAY: Duration = Duration::from_millis(80);
    /// After pasting with the panel kept open, the panel comes back to the front this much later.
    pub const RAISE_DELAY: Duration = Duration::from_millis(150);
    /// While pasting with the panel kept open, losing focus does not close it for this long.
    pub const BLUR_GRACE: Duration = Duration::from_millis(800);
    /// Losing focus right after opening is ignored; the window is still settling.
    pub const BLUR_IGNORE_AFTER_SHOW: Duration = Duration::from_millis(300);
    /// Whether the panel really lost focus is looked at this long after the window reports it.
    pub const BLUR_CHECK_DELAY: Duration = Duration::from_millis(100);
    /// The daemon is asked again this long after it did not answer.
    pub const RECONNECT_DELAY: Duration = Duration::from_secs(3);
    /// The event stream is subscribed again this long after it ended.
    pub const LIVE_RESUBSCRIBE_DELAY: Duration = Duration::from_secs(2);
}

/// What the state needs to know about the world when it is asked to do something.
pub struct Ctx<'a> {
    pub now: Instant,
    /// The application a paste would go to.
    pub target: &'a dyn PasteTarget,
    pub capabilities: Capabilities,
    /// The text in the search box.
    pub input: &'a str,
    /// An input method is composing text in the search box.
    pub composing: bool,
    /// The search box has selected text.
    pub selecting: bool,
    pub today: NaiveDate,
}

/// What the shell has to do. The state never does these itself.
#[derive(Debug)]
pub enum Effect {
    // Windows
    /// Places the panel window where it belongs and sizes it.
    Reposition,
    /// Sizes the panel window again.
    Layout,
    /// Captures the application in front as the paste target, before the panel covers it.
    CaptureTarget,
    ApplyTheme,
    ShowWindow,
    /// Hides the window and reports [`Event::WindowHidden`] or [`Event::WindowFailed`].
    HideWindow,
    /// Brings the window to the front again; nothing is reported if that fails.
    RaiseWindow,
    ReturnFocus,
    ShowPreviewWindow,
    HidePreviewWindow,
    // Search box and list
    /// Empties the search box and focuses it.
    ResetInput,
    /// Puts text in the search box and focuses it.
    SetInput(String),
    FocusInput,
    ScrollListToTop,
    ScrollToItem(usize),
    ClearImages,
    ClearImageBounds,
    ClearPreviewAnchor,
    // Waiting work; each kind replaces the one before it
    Search {
        revision: u64,
        filters: Filters,
        debounce: Duration,
    },
    CancelSearch,
    CountRelaxations {
        revision: u64,
        queries: Vec<Filters>,
    },
    ScheduleReconnect(Duration),
    CancelReconnect,
    LoadOptions,
    LoadThumbnails,
    SchedulePreview {
        id: String,
        kind: String,
        delay: Duration,
    },
    LoadPreview {
        id: String,
        kind: String,
    },
    CancelPreview,
    StoreImage {
        id: String,
        payload: ImagePayload,
        size: i64,
    },
    ScheduleBlurCheck(Duration),
    CancelBlurCheck,
    // The clipboard and the target application
    Restore {
        id: String,
        plain: bool,
        paste: bool,
        keep_open: bool,
    },
    /// Pastes into the target after `delay`.
    PasteToTarget {
        keep_open: bool,
        delay: Duration,
    },
    TypeText {
        text: String,
        delay: Duration,
    },
    EntryAction {
        id: String,
        action: EntryAction,
    },
    // Outside the panel
    OpenTarget(String),
    RevealPath(String),
    OpenLogs,
    HostRequest(HostRequest),
    /// Applies what the settings say to the shell: shortcuts, the double-tap trigger, the theme.
    ApplySettings(Box<Option<SettingsDto>>),
}

/// What became of an effect, or what happened that the state has to know about.
#[derive(Debug)]
pub enum Event {
    WindowHidden,
    WindowFailed(PlatformError),
    /// The search box changed to this text.
    InputChanged(String),
    SearchDone {
        revision: u64,
        result: Result<SearchQueryResultDto, SearchFailure>,
    },
    CountsDone {
        revision: u64,
        totals: Vec<Option<u32>>,
    },
    OptionsLoaded(Result<Box<Options>, ServiceError>),
    Live(Live),
    ReconnectDue,
    PreviewDue {
        id: String,
        kind: String,
    },
    PreviewLoaded {
        id: String,
        result: Result<PreviewData, ServiceError>,
    },
    Restored {
        result: Result<(), ServiceError>,
        paste: bool,
        keep_open: bool,
    },
    /// The paste, or the typing of file paths, did not go through.
    PasteFailed(PlatformError),
    /// The paste went through.
    PasteDelivered {
        keep_open: bool,
    },
    ActionDone(Result<(), ServiceError>),
    Opened(Result<(), PlatformError>),
    Revealed(Result<(), PlatformError>),
    HostRequested(Result<(), String>),
    /// The panel window lost the focus.
    Deactivated,
    /// The panel window got the focus.
    Activated,
    /// The delayed look at whether the focus is really gone.
    BlurChecked {
        panel_active: bool,
        preview_active: bool,
    },
}

/// The effects one call asks for, in the order they are to be carried out.
pub type Effects = Vec<Effect>;

#[derive(Debug)]
pub struct Session {
    pub visible: bool,
    pub shown_at: Instant,
    /// While pasting with the panel kept open, the target is in front for a moment; losing focus
    /// then does not close the panel.
    pub blur_grace_until: Option<Instant>,
    /// An action on an entry is running; input is ignored meanwhile.
    pub busy: bool,
    pub message: Option<String>,
}

pub struct Search {
    pub filters: Filters,
    pub items: Vec<SearchResultDto>,
    pub total: u32,
    /// Counts searches, so an answer to an old one can be told from the current one.
    pub revision: u64,
    pub loading: bool,
    pub locked: bool,
    /// Failed attempts to reach the daemon since it last answered.
    pub disconnected: Option<u32>,
    pub selection: Selection,
    pub hovered: Option<usize>,
    /// The keyboard drives the selection, so the pointer does not.
    pub keyboard: bool,
    pub pointer_moved: bool,
    /// First visible row of the image grid; the grid shows three rows from here.
    pub grid_top: usize,
    /// Ways to loosen a search that found nothing, each with how many entries it would show.
    pub relaxations: Vec<(Relaxation, Option<u32>)>,
    pub relax_cursor: usize,
}

#[derive(Default)]
pub struct Preview {
    pub entry: Option<String>,
    pub text: Option<String>,
    pub size: i64,
    pub loading: bool,
    pub expanded: bool,
}

#[derive(Default)]
pub struct Suggestions {
    pub open: bool,
    pub cursor: usize,
    /// The arrow keys are in the suggestion list rather than in the results.
    pub focused: bool,
}

pub struct Catalog {
    pub tags: Vec<String>,
    pub members: Vec<SpaceMemberDto>,
    /// The panel opens next to the pointer instead of in the middle of the screen.
    pub cursor_anchored: bool,
}

pub struct PanelState {
    pub session: Session,
    pub search: Search,
    pub preview: Preview,
    pub menu: Option<ActionMenu>,
    pub suggest: Suggestions,
    pub catalog: Catalog,
}

fn builtin_tags() -> Vec<String> {
    filters::BUILTIN_TAGS.iter().map(|s| (*s).into()).collect()
}

impl PanelState {
    pub fn new(now: Instant) -> Self {
        Self {
            session: Session {
                visible: false,
                shown_at: now,
                blur_grace_until: None,
                busy: false,
                message: None,
            },
            search: Search {
                filters: Filters::default(),
                items: vec![],
                total: 0,
                revision: 0,
                loading: false,
                locked: false,
                disconnected: None,
                selection: Selection::default(),
                hovered: None,
                keyboard: true,
                pointer_moved: false,
                grid_top: 0,
                relaxations: vec![],
                relax_cursor: 0,
            },
            preview: Preview::default(),
            menu: None,
            suggest: Suggestions::default(),
            catalog: Catalog {
                tags: builtin_tags(),
                members: vec![],
                cursor_anchored: false,
            },
        }
    }

    /// What to do when the panel process starts, before the panel is ever shown.
    pub fn start(&mut self) -> Effects {
        let mut effects = vec![Effect::Reposition, Effect::LoadOptions];
        effects.extend(self.search());
        effects
    }

    /// The entry the selection or the pointer is on.
    pub fn active_index(&self) -> Option<usize> {
        self.search.hovered.or(self.search.selection.selected())
    }

    pub fn active_item(&self) -> Option<&SearchResultDto> {
        self.active_index().and_then(|ix| self.search.items.get(ix))
    }

    /// The name of a device, or its id when it is not known.
    pub fn device_name(&self, id: &str) -> String {
        self.catalog
            .members
            .iter()
            .find(|member| member.peer_id == id)
            .map_or_else(|| id.to_string(), |member| member.device_name.clone())
    }

    /// The rows the action list shows now, with their title.
    pub fn action_rows(&self, ctx: &Ctx) -> Option<(String, Vec<Row>)> {
        menu::rows_of(self, ctx)
    }

    /// The suggestions worth showing: those that would find something, or are still being counted.
    pub fn visible_relaxations(&self) -> Vec<&(Relaxation, Option<u32>)> {
        self.search
            .relaxations
            .iter()
            .filter(|(_, count)| *count != Some(0))
            .collect()
    }

    pub fn set_message(&mut self, message: impl Into<String>) {
        self.session.message = Some(message.into());
    }

    /// Applies what happened to an effect. Returns the effects that follow from it.
    pub fn on_event(&mut self, event: Event, ctx: &Ctx) -> Effects {
        match event {
            Event::WindowHidden => self.on_hidden(),
            Event::WindowFailed(error) => {
                self.session.message = Some(error.to_string());
                vec![]
            }
            Event::InputChanged(value) => self.input_changed(value, ctx),
            Event::SearchDone { revision, result } => self.on_search_done(revision, result),
            Event::CountsDone { revision, totals } => {
                self.on_counts(revision, totals);
                vec![]
            }
            Event::OptionsLoaded(result) => self.on_options(result),
            Event::Live(live) => self.on_live(live),
            Event::ReconnectDue => self.on_reconnect_due(),
            Event::PreviewDue { id, kind } => self.on_preview_due(id, kind),
            Event::PreviewLoaded { id, result } => self.on_preview_loaded(id, result),
            Event::Restored {
                result,
                paste,
                keep_open,
            } => self.on_restored(result, paste, keep_open, ctx),
            Event::PasteFailed(error) => self.on_paste_failed(error, ctx),
            Event::PasteDelivered { keep_open } => self.on_paste_delivered(keep_open),
            Event::ActionDone(result) => self.on_action_done(result),
            Event::Opened(result) => self.on_opened(result),
            Event::Revealed(result) => {
                if let Err(error) = result {
                    self.session.message = Some(error.to_string());
                }
                vec![]
            }
            Event::HostRequested(result) => match result {
                Ok(()) => vec![Effect::HideWindow],
                Err(message) => {
                    self.session.message = Some(message);
                    vec![]
                }
            },
            Event::Deactivated => self.on_deactivated(ctx),
            Event::Activated => vec![Effect::CancelBlurCheck],
            Event::BlurChecked {
                panel_active,
                preview_active,
            } => self.on_blur_checked(panel_active, preview_active, ctx),
        }
    }
}
