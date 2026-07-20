//! Shared assembly of an outbound clipboard payload for the user/peer-initiated
//! send paths (resend + active-clipboard pull serve).
//!
//! Both paths run the identical "resolve file set → read metadata → plan →
//! publish blobs → build directory manifest → stamp identity → publish
//! oversized inline reps" sequence and differ only in how they map the failure
//! modes onto their own error surfaces. Keeping the sequence here (next to
//! [`resolve_outbound_file_set`]) prevents the parallel-logic drift that let
//! the pull-serve path leak flat directory frames in the first place (issue
//! #1327).
//!
//! The `dispatch_capture` path intentionally does NOT go through this helper:
//! its orchestration is genuinely different (LocalCapture origin gating,
//! per-phase latency instrumentation for GH#487, `Skipped { reason }`
//! outcomes), and folding it in would turn this into a leaky mega-helper.

use std::sync::Arc;

use tracing::warn;

use uc_core::ids::EntryId;
use uc_core::ports::clipboard::EntryFileSetRepositoryPort;
use uc_core::ports::SettingsPort;
use uc_core::{ClipboardChangeOrigin, SystemClipboardSnapshot};

use crate::sync_planner::{FileCandidate, OutboundSyncPlanner};
use crate::usecases::clipboard_sync::apply_inbound::{
    compute_file_set_component, InboundFileSetManifest,
};
use crate::usecases::clipboard_sync::V3BlobRef;

use super::{
    build_transfer_manifest, display_name_for_path, file_display_metadata, publish_file_blob_refs,
    publish_oversized_inline_blob_refs, resolve_outbound_file_set, ClipboardOutboundError,
    OutboundBlobPublishGateway, OutboundFileSetResolution,
};

/// The assembled outbound payload: the (possibly mutated) snapshot with its
/// identity fields stamped, the published blob refs, and the directory
/// manifest when the entry is a directory set (`None` for flat/text entries —
/// the caller picks the matching V3 encoder from its presence).
pub(crate) struct OutboundPayload {
    pub snapshot: SystemClipboardSnapshot,
    pub blob_refs: Vec<V3BlobRef>,
    pub file_set_manifest: Option<InboundFileSetManifest>,
}

/// Failure surface of [`assemble_outbound_payload`]. Details are logged at the
/// failure site; callers only map the class onto their own error enums.
pub(crate) enum OutboundPayloadError {
    /// This device cannot reproduce the payload in full: the manifest has
    /// excluded lines, a manifest member is unreadable, the planner dropped a
    /// directory member, or every referenced file is gone (all-or-nothing).
    Unavailable,
    /// A blob publish step failed.
    Publish(ClipboardOutboundError),
    /// Manifest construction or identity-component computation failed.
    Internal(String),
}

/// Assemble the outbound payload for a user/peer-initiated send of `entry_id`.
///
/// Plans with [`ClipboardChangeOrigin::Resend`] — both callers serve content
/// the user already holds, bypassing the `max_file_size` capture guard.
/// Manifest-backed file sets are all-or-nothing: any member this device cannot
/// publish fails the whole payload ([`OutboundPayloadError::Unavailable`])
/// instead of shipping a subset with a diverged identity.
pub(crate) async fn assemble_outbound_payload(
    entry_file_set_repo: &dyn EntryFileSetRepositoryPort,
    blob_publisher: &dyn OutboundBlobPublishGateway,
    settings: Arc<dyn SettingsPort>,
    entry_id: &EntryId,
    snapshot: SystemClipboardSnapshot,
) -> Result<OutboundPayload, OutboundPayloadError> {
    let display_metadata = file_display_metadata(&snapshot);
    // 1. Member path list from the persisted file-set manifest — the single
    //    source of truth shared with the dispatch path.
    let resolution = resolve_outbound_file_set(entry_file_set_repo, entry_id, &snapshot).await;
    let (resolved_paths, from_manifest, expected_digests, directory_members) = match resolution {
        OutboundFileSetResolution::NotFileClass => (Vec::new(), false, Vec::new(), None),
        OutboundFileSetResolution::Manifest {
            paths,
            expected_digests,
        } => (paths, true, expected_digests, None),
        OutboundFileSetResolution::Fallback { paths } => (paths, false, Vec::new(), None),
        OutboundFileSetResolution::DirectorySyncable { paths, members } => {
            (paths, true, Vec::new(), Some(members))
        }
        OutboundFileSetResolution::Excluded {
            ingest_failed,
            size_cap_exceeded,
            unsupported_member,
        } => {
            warn!(
                entry_id = %entry_id,
                ingest_failed,
                size_cap_exceeded,
                unsupported_member,
                "outbound payload: file-set manifest has excluded lines; not reproducible (all-or-nothing)"
            );
            return Err(OutboundPayloadError::Unavailable);
        }
    };

    // 2. Read metadata. A manifest member unreadable now means this device can
    //    no longer reproduce the set the entry's identity covers; legacy
    //    (fallback) sets keep the lenient warn-and-skip semantics.
    let extracted_paths_count = resolved_paths.len();
    let mut file_candidates = Vec::with_capacity(resolved_paths.len());
    for path in resolved_paths {
        match tokio::fs::metadata(&path).await {
            Ok(meta) => file_candidates.push(FileCandidate {
                display_name: display_name_for_path(display_metadata.as_ref(), &path),
                path,
                size: meta.len(),
            }),
            Err(err) if from_manifest => {
                warn!(
                    error = %err,
                    entry_id = %entry_id,
                    "outbound payload: file-set member unreadable; not reproducible (all-or-nothing)"
                );
                return Err(OutboundPayloadError::Unavailable);
            }
            Err(err) => warn!(
                error = %err,
                entry_id = %entry_id,
                "outbound payload: excluding clipboard file whose metadata could not be read"
            ),
        }
    }

    // 3. Plan. The only branch that drops `clipboard` for a Resend-origin plan
    //    is `all_files_excluded` (every referenced file gone / over caps).
    let planner = OutboundSyncPlanner::new(settings);
    let plan = planner
        .plan(
            snapshot,
            ClipboardChangeOrigin::Resend,
            file_candidates,
            extracted_paths_count,
        )
        .await;
    let Some(mut clipboard_intent) = plan.clipboard else {
        warn!(
            entry_id = %entry_id,
            "outbound payload: planner excluded every referenced file; not reproducible"
        );
        return Err(OutboundPayloadError::Unavailable);
    };

    // All-or-nothing for every manifest-backed set (flat AND directory): a
    // planner-dropped member (e.g. file sync disabled → zero files planned)
    // means the wire payload cannot carry the contents the entry's persisted
    // identity covers. Serving the remainder would ship path text under a
    // content-keyed identity — refuse instead.
    if from_manifest && plan.files.len() != extracted_paths_count {
        warn!(
            entry_id = %entry_id,
            published = plan.files.len(),
            expected = extracted_paths_count,
            "outbound payload: planner excluded manifest members; not reproducible (all-or-nothing)"
        );
        return Err(OutboundPayloadError::Unavailable);
    }

    // 4. Publish file blobs (re-issues tickets pinned to this device).
    let (mut blob_refs, file_content_digests) =
        publish_file_blob_refs(blob_publisher, &plan.files, entry_id)
            .await
            .map_err(OutboundPayloadError::Publish)?;

    // Capture→send drift observability (flat manifests only — directory sets
    // carry no flat digest contribution): the manifest digests are the
    // identity the entry was persisted under; the publish digests are what
    // actually goes on the wire. The all-or-nothing guard above already
    // ensured the planner kept every member.
    if !expected_digests.is_empty() {
        let mut wire_digests = file_content_digests.clone();
        wire_digests.sort_unstable();
        wire_digests.dedup();
        let mut capture_digests = expected_digests;
        capture_digests.dedup();
        if wire_digests != capture_digests {
            warn!(
                entry_id = %entry_id,
                "outbound payload: file content drifted between capture and send; wire identity keyed on current bytes"
            );
        }
    }

    // 5. Directory sets: the transfer manifest + structured identity component
    //    replace the flat digest contribution (member completeness is already
    //    guaranteed by the all-or-nothing guard above).
    let file_set_manifest = match directory_members {
        Some(members) => Some(
            build_transfer_manifest(&members, &plan.files)
                .map_err(|err| OutboundPayloadError::Internal(err.to_string()))?,
        ),
        None => None,
    };
    if let Some(manifest) = &file_set_manifest {
        let digests = file_content_digests
            .iter()
            .enumerate()
            .map(|(index, digest)| (index as u32, *digest))
            .collect();
        clipboard_intent.snapshot.file_content_digests.clear();
        clipboard_intent.snapshot.file_set_v1_component = Some(
            compute_file_set_component(manifest, &digests)
                .map_err(|err| OutboundPayloadError::Internal(err.to_string()))?,
        );
    } else if !file_content_digests.is_empty() {
        clipboard_intent.snapshot.file_content_digests = file_content_digests;
    }

    // 6. Publish oversized inline (image) reps.
    let mut image_blob_refs = publish_oversized_inline_blob_refs(
        blob_publisher,
        &mut clipboard_intent.snapshot,
        entry_id,
    )
    .await
    .map_err(OutboundPayloadError::Publish)?;
    blob_refs.append(&mut image_blob_refs);

    Ok(OutboundPayload {
        snapshot: clipboard_intent.snapshot,
        blob_refs,
        file_set_manifest,
    })
}
