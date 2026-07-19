//! Product analytics data and observer interfaces.

pub mod context;
pub mod events;
pub mod facade;
pub mod identity;
pub mod port;

pub use context::Os;
pub use events::{
    CaptureOrigin, DialogOpenSource, Direction, DismissSource, Event, FailureReason, InstallKind,
    InvitationCodeSource, LatencyBucket, MobileAuthFailureKind, NameLengthBucket,
    NotificationDeliveryStatus, PairingDiscoveryChannel, PairingFailureReason, PairingMethod,
    PayloadSizeBucket, PayloadType, SetupEntry, SyncDeferReason, SyncDeferredProps, SyncEventProps,
    SyncFailureStage, TransportType, UnlockFailureReason, UpdateAction, UpdateActionOutcome,
    UpdateCheckOutcome, UpdateCheckSource, UpdateFailureKind, UpdatePhase,
};
pub use facade::{
    AnalyticsFacade, DefaultAnalyticsFacade, NoopAnalyticsFacade, ResetIdentityError,
    SelfMintedAdoptRequest,
};
pub use identity::{
    hash_space_id_for_telemetry, AdoptOutcome, AnalyticsIdentityError, AnalyticsIdentityPort,
    NoopAnalyticsIdentity, ReleaseOutcome,
};
pub use port::{AnalyticsPort, GroupIdentifyPayload, IdentifyPayload, NoopAnalyticsSink};
