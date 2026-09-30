//! The clipboard history behind the panel: search, restore, entry actions and live changes.

use std::fmt;

use async_trait::async_trait;
use uc_daemon_contract::api::dto::search::SearchQueryResultDto;
use uc_daemon_contract::api::dto::settings::SettingsDto;
use uc_daemon_contract::api::types::SpaceMemberDto;

use crate::query::filters::Filters;
use crate::text::service as t;

/// Why a search did not produce a result list. The `Display` text is what the user is shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchFailure {
    /// The history is locked and needs unlocking in the main window.
    Locked,
    Failed,
    /// The daemon cannot be reached at all: no connection info, a refused connection or a failed
    /// authorization. Unlike `Failed` it is worth retrying by itself.
    Disconnected,
    /// The request was abandoned before an answer came back.
    Interrupted,
    Timeout,
}

impl fmt::Display for SearchFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Locked => t::LOCKED,
            Self::Failed => t::SEARCH_FAILED,
            Self::Disconnected => t::DISCONNECTED,
            Self::Interrupted => t::INTERRUPTED,
            Self::Timeout => t::SEARCH_TIMEOUT,
        })
    }
}

impl std::error::Error for SearchFailure {}

/// Why an operation on the history failed. The `Display` text is what the user is shown; details
/// of the remote side never appear in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceError {
    /// No daemon connection information is available.
    NotRunning,
    CannotConnect,
    RestoreFailed,
    RestoreTimeout,
    ActionFailed,
    TagsUnavailable,
    DevicesUnavailable,
    PreviewTimeout,
    PreviewUnreadable,
    EntryGone,
    ImageUnreadable,
    ImageGone,
    ImageBadFormat,
    ImageUnsupported,
    ImageUndecodable,
}

impl fmt::Display for ServiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NotRunning => t::NOT_RUNNING,
            Self::CannotConnect => t::CANNOT_CONNECT,
            Self::RestoreFailed => t::RESTORE_FAILED,
            Self::RestoreTimeout => t::RESTORE_TIMEOUT,
            Self::ActionFailed => t::ACTION_FAILED,
            Self::TagsUnavailable => t::TAGS_UNAVAILABLE,
            Self::DevicesUnavailable => t::DEVICES_UNAVAILABLE,
            Self::PreviewTimeout => t::PREVIEW_TIMEOUT,
            Self::PreviewUnreadable => t::PREVIEW_UNREADABLE,
            Self::EntryGone => t::ENTRY_GONE,
            Self::ImageUnreadable => t::IMAGE_UNREADABLE,
            Self::ImageGone => t::IMAGE_GONE,
            Self::ImageBadFormat => t::IMAGE_BAD_FORMAT,
            Self::ImageUnsupported => t::IMAGE_UNSUPPORTED,
            Self::ImageUndecodable => t::IMAGE_UNDECODABLE,
        })
    }
}

impl std::error::Error for ServiceError {}

/// Filter choices and settings read when the panel opens.
#[derive(Debug)]
pub struct Options {
    pub tags: Vec<String>,
    pub members: Vec<SpaceMemberDto>,
    pub settings: Option<SettingsDto>,
}

/// An action on one entry, other than restoring it to the clipboard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryAction {
    Favorite(bool),
    Delete,
    /// Sends the entry to one device, or to every device when `None`.
    Send(Option<String>),
}

/// An encoded image with its size in pixels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImagePayload {
    pub bytes: Vec<u8>,
    pub mime: String,
    pub width: u32,
    pub height: u32,
}

/// What a preview needs: text for text entries, an image for images.
#[derive(Debug)]
pub struct PreviewData {
    pub text: Option<String>,
    pub image: Option<ImagePayload>,
    pub size: i64,
}

/// What the daemon's event stream tells the panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Live {
    /// Something changed; search again if the panel is open.
    Changed,
    /// Content got locked: drop everything held, whether the panel is shown or not.
    ContentLocked,
    /// Content got unlocked: search again if the panel is open.
    ContentUnlocked,
}

#[async_trait]
pub trait HistoryService: Send + Sync {
    async fn search(&self, filters: Filters) -> Result<SearchQueryResultDto, SearchFailure>;

    /// How many entries `filters` would find, if that can be told.
    async fn count(&self, filters: Filters) -> Option<u32>;

    /// Puts an entry back on the clipboard, as plain text when `plain` is set.
    async fn restore(&self, id: String, plain: bool) -> Result<(), ServiceError>;

    async fn options(&self) -> Result<Options, ServiceError>;

    async fn action(&self, id: String, action: EntryAction) -> Result<(), ServiceError>;

    /// The preview of an entry of content type `kind`.
    async fn preview(&self, id: String, kind: String) -> Result<PreviewData, ServiceError>;

    /// Forwards live changes to `send` until it is closed or the stream ends. A lock waits for
    /// room in the channel instead of being dropped.
    async fn watch(&self, send: tokio::sync::mpsc::Sender<Live>);
}
