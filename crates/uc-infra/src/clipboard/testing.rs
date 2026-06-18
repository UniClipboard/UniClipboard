//! Test-only helpers for the `clipboard` module.
//!
//! Why a hand-rolled fake instead of `mockall`: `ClipboardRepresentationStore`
//! takes `Option<&BlobId>` and `Option<&str>`, which can't be matched by `mockall::mock!`
//! without rewriting the trait signature with explicit lifetimes (mockall 0.13 rejects
//! method-level lifetime params that don't match the trait declaration). Project-wide
//! we use mockall, but this single trait stays on a hand-rolled fake so the spool /
//! reconciler tests have a uniform fixture.

#![cfg(test)]

use std::sync::Mutex;

use anyhow::Result;
use async_trait::async_trait;
use uc_core::clipboard::{MimeType, PayloadAvailability, PersistedClipboardRepresentation};
use uc_core::ids::{EventId, RepresentationId};
use uc_core::ports::clipboard::{ClipboardRepresentationStore, ProcessingUpdateOutcome};
use uc_core::BlobId;

/// A scripted return value for one call to `update_processing_result`.
pub enum ScriptedReturn {
    Ok(ProcessingUpdateOutcome),
    Err(String),
}

/// Owned snapshot of a single `update_processing_result` invocation.
#[allow(dead_code)] // Some fields are only asserted by a subset of tests.
pub struct UpdateProcessingCall {
    pub rep_id: RepresentationId,
    pub expected_states: Vec<PayloadAvailability>,
    pub blob_id: Option<BlobId>,
    pub new_state: PayloadAvailability,
    pub last_error: Option<String>,
}

/// Hand-rolled fake for `ClipboardRepresentationStore`.
///
/// - `update_processing_result` consumes one scripted outcome per call (FIFO),
///   panicking if the script is empty — that's the same "unexpected call" signal
///   mockall gives.
/// - Other methods follow the trait's default impls (return empty / ok), with
///   `unimplemented!()` on the few that have no default and aren't exercised here.
/// - `list_ids_by_payload_state` reads from `staged_ids` so reconciler tests
///   can seed candidate lists.
pub struct ScriptedRepRepo {
    update_outcomes: Mutex<Vec<ScriptedReturn>>,
    update_calls: Mutex<Vec<UpdateProcessingCall>>,
    staged_ids: Mutex<Vec<RepresentationId>>,
    rep_by_id: Mutex<Option<PersistedClipboardRepresentation>>,
}

impl ScriptedRepRepo {
    pub fn new() -> Self {
        Self {
            update_outcomes: Mutex::new(Vec::new()),
            update_calls: Mutex::new(Vec::new()),
            staged_ids: Mutex::new(Vec::new()),
            rep_by_id: Mutex::new(None),
        }
    }

    /// Seed the representation returned by `get_representation_by_id`.
    pub fn set_representation(&self, rep: PersistedClipboardRepresentation) {
        *self.rep_by_id.lock().unwrap() = Some(rep);
    }

    pub fn push_update_outcome(&self, ret: ScriptedReturn) {
        self.update_outcomes.lock().unwrap().push(ret);
    }

    pub fn set_staged_ids(&self, ids: Vec<RepresentationId>) {
        *self.staged_ids.lock().unwrap() = ids;
    }

    pub fn update_processing_calls(&self) -> Vec<UpdateProcessingCall> {
        std::mem::take(&mut *self.update_calls.lock().unwrap())
    }
}

#[async_trait]
impl ClipboardRepresentationStore for ScriptedRepRepo {
    async fn get_representation(
        &self,
        _event_id: &EventId,
        _representation_id: &RepresentationId,
    ) -> Result<Option<PersistedClipboardRepresentation>> {
        unimplemented!("ScriptedRepRepo: get_representation not configured for this test")
    }

    async fn get_representation_by_id(
        &self,
        _representation_id: &RepresentationId,
    ) -> Result<Option<PersistedClipboardRepresentation>> {
        match self.rep_by_id.lock().unwrap().clone() {
            Some(rep) => Ok(Some(rep)),
            None => {
                unimplemented!("ScriptedRepRepo: set_representation() not called for this test")
            }
        }
    }

    async fn get_representation_by_blob_id(
        &self,
        _blob_id: &BlobId,
    ) -> Result<Option<PersistedClipboardRepresentation>> {
        unimplemented!(
            "ScriptedRepRepo: get_representation_by_blob_id not configured for this test"
        )
    }

    async fn update_blob_id(
        &self,
        _representation_id: &RepresentationId,
        _blob_id: &BlobId,
    ) -> Result<()> {
        unimplemented!("ScriptedRepRepo: update_blob_id not configured for this test")
    }

    async fn update_blob_id_if_none(
        &self,
        _representation_id: &RepresentationId,
        _blob_id: &BlobId,
    ) -> Result<bool> {
        unimplemented!("ScriptedRepRepo: update_blob_id_if_none not configured for this test")
    }

    async fn update_processing_result(
        &self,
        rep_id: &RepresentationId,
        expected_states: &[PayloadAvailability],
        blob_id: Option<&BlobId>,
        new_state: PayloadAvailability,
        last_error: Option<&str>,
    ) -> Result<ProcessingUpdateOutcome> {
        self.update_calls
            .lock()
            .unwrap()
            .push(UpdateProcessingCall {
                rep_id: rep_id.clone(),
                expected_states: expected_states.to_vec(),
                blob_id: blob_id.cloned(),
                new_state: new_state.clone(),
                last_error: last_error.map(|s| s.to_string()),
            });

        let mut script = self.update_outcomes.lock().unwrap();
        if script.is_empty() {
            panic!("ScriptedRepRepo: update_processing_result called more times than scripted");
        }
        match script.remove(0) {
            ScriptedReturn::Ok(outcome) => Ok(outcome),
            ScriptedReturn::Err(msg) => Err(anyhow::anyhow!(msg)),
        }
    }

    async fn get_representations_for_event(
        &self,
        _event_id: &EventId,
    ) -> Result<Vec<PersistedClipboardRepresentation>> {
        Ok(vec![])
    }

    async fn update_mime_type(&self, _rep_id: &RepresentationId, _mime: &MimeType) -> Result<()> {
        Ok(())
    }

    async fn list_ids_by_payload_state(
        &self,
        _states: &[PayloadAvailability],
    ) -> Result<Vec<RepresentationId>> {
        Ok(self.staged_ids.lock().unwrap().clone())
    }
}
