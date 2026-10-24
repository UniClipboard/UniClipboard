//! The clipboard history behind the panel: search, restore, entry actions and live changes.

use std::fmt;

use async_trait::async_trait;
use uc_daemon_contract::api::dto::search::SearchQueryResultDto;
use uc_daemon_contract::api::dto::settings::SettingsDto;
use uc_daemon_contract::api::types::SpaceMemberDto;

use crate::query::filters::Filters;

/// Why a search did not produce a result list. The `Display` text is what the user is shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchFailure {
    /// The history is locked and needs unlocking.
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
            Self::Locked => crate::text::t().service.locked,
            Self::Failed => crate::text::t().service.search_failed,
            Self::Disconnected => crate::text::t().service.disconnected,
            Self::Interrupted => crate::text::t().service.interrupted,
            Self::Timeout => crate::text::t().service.search_timeout,
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
            Self::NotRunning => crate::text::t().service.not_running,
            Self::CannotConnect => crate::text::t().service.cannot_connect,
            Self::RestoreFailed => crate::text::t().service.restore_failed,
            Self::RestoreTimeout => crate::text::t().service.restore_timeout,
            Self::ActionFailed => crate::text::t().service.action_failed,
            Self::TagsUnavailable => crate::text::t().service.tags_unavailable,
            Self::DevicesUnavailable => crate::text::t().service.devices_unavailable,
            Self::PreviewTimeout => crate::text::t().service.preview_timeout,
            Self::PreviewUnreadable => crate::text::t().service.preview_unreadable,
            Self::EntryGone => crate::text::t().service.entry_gone,
            Self::ImageUnreadable => crate::text::t().service.image_unreadable,
            Self::ImageGone => crate::text::t().service.image_gone,
            Self::ImageBadFormat => crate::text::t().service.image_bad_format,
            Self::ImageUnsupported => crate::text::t().service.image_unsupported,
            Self::ImageUndecodable => crate::text::t().service.image_undecodable,
        })
    }
}

impl std::error::Error for ServiceError {}

/// A passphrase on its way to the daemon. It never prints: `Debug` hides it, so a logged effect or
/// event cannot leak it.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(..)")
    }
}

/// Why the daemon did not unlock the content. The `Display` text is what the user is shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnlockFailure {
    WrongPassphrase,
    /// Setup is not finished or the space is not initialized: there is nothing to unlock yet.
    NotReady,
    /// The stored key material cannot be read; typing the passphrase again will not help.
    KeyUnreadable,
    Disconnected,
    Failed,
    Timeout,
}

impl fmt::Display for UnlockFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let service = &crate::text::t().service;
        f.write_str(match self {
            Self::WrongPassphrase => service.unlock_wrong_passphrase,
            Self::NotReady => service.unlock_not_ready,
            Self::KeyUnreadable => service.unlock_key_unreadable,
            Self::Disconnected => service.disconnected,
            Self::Failed => service.unlock_failed,
            Self::Timeout => service.unlock_timeout,
        })
    }
}

impl std::error::Error for UnlockFailure {}

/// Filter choices and settings read when the panel opens.
#[derive(Debug)]
pub struct Options {
    /// Read on their own: the daemon refuses tags and devices while the content is locked, but
    /// still serves the settings the locked page is drawn with.
    pub choices: Result<Choices, ServiceError>,
    pub settings: Option<SettingsDto>,
}

/// The tags and devices to filter by.
#[derive(Debug)]
pub struct Choices {
    pub tags: Vec<String>,
    pub members: Vec<SpaceMemberDto>,
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

    /// Checks the passphrase with the daemon and, when it is right, lets this panel show content.
    async fn unlock(&self, passphrase: Secret) -> Result<(), UnlockFailure>;

    /// Tries the system secure store after an explicit user action. False requests a passphrase.
    async fn unlock_from_keyring(&self) -> Result<bool, UnlockFailure>;

    /// Forwards live changes to `send` until it is closed or the stream ends. A lock waits for
    /// room in the channel instead of being dropped.
    async fn watch(&self, send: tokio::sync::mpsc::Sender<Live>);
}
