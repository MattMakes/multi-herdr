//! Recording events (dataset design §4.2).
//!
//! Normal `horch` mode records nothing: it holds a [`NoopRecorder`]. The
//! dataset entrypoint holds a [`JsonlRecorder`], which appends to the event
//! log and is idempotent: an event whose key is already in the log writes
//! nothing and returns the earlier envelope (MEA-04).

use std::sync::Mutex;

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};

use crate::ids::{EventId, ExecutionId, ExperimentId, RoundId};
use crate::measure::event::{
    format_occurred_at, Actor, EventEnvelope, EventKind, EVENT_SCHEMA_VERSION,
};
use crate::measure::paths::DatasetPaths;
use crate::measure::store::{self, EventIndex, ReadEvents, StoreOptions};

/// An event before it has an id and an envelope.
#[derive(Debug, Clone, PartialEq)]
pub struct NewEvent {
    pub kind: EventKind,
    pub actor: Actor,
    pub experiment_id: ExperimentId,
    pub round_id: Option<RoundId>,
    pub execution_id: Option<ExecutionId>,
    /// Names the effect, such as `spawn:<round>:<label>`; one event per key.
    pub idempotency_key: String,
    pub occurred_at: DateTime<Utc>,
}

impl NewEvent {
    /// The envelope, with a fresh v7 id minted at `occurred_at`.
    pub fn into_envelope(self) -> Result<EventEnvelope> {
        if self.idempotency_key.is_empty() {
            bail!("event '{}' has an empty idempotency key", self.kind.name());
        }
        let payload = self.kind.try_payload()?;
        Ok(EventEnvelope {
            schema_version: EVENT_SCHEMA_VERSION.to_string(),
            event_id: EventId::mint(self.occurred_at),
            kind: self.kind.name().to_string(),
            occurred_at: format_occurred_at(self.occurred_at),
            actor: self.actor,
            experiment_id: self.experiment_id,
            round_id: self.round_id,
            execution_id: self.execution_id,
            idempotency_key: self.idempotency_key,
            payload,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Appended {
    Recorded(EventEnvelope),
    /// The earlier envelope with the same key. Nothing was written.
    Duplicate(EventEnvelope),
}

impl Appended {
    pub fn envelope(&self) -> &EventEnvelope {
        match self {
            Appended::Recorded(env) | Appended::Duplicate(env) => env,
        }
    }
}

pub trait Recorder {
    fn append(&self, event: NewEvent) -> Result<Appended>;
}

/// Records nothing. Normal `horch` mode uses it.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoopRecorder;

impl Recorder for NoopRecorder {
    fn append(&self, event: NewEvent) -> Result<Appended> {
        Ok(Appended::Recorded(event.into_envelope()?))
    }
}

/// The JSONL event log of one dataset.
#[derive(Debug)]
pub struct JsonlRecorder {
    paths: DatasetPaths,
    options: StoreOptions,
    index: Mutex<EventIndex>,
}

impl JsonlRecorder {
    /// Create the dataset dirs and rebuild the idempotency index from every
    /// event file.
    pub fn open(paths: &DatasetPaths, options: StoreOptions) -> Result<JsonlRecorder> {
        paths.ensure().context("creating the dataset dirs")?;
        let mut index = EventIndex::default();
        index.refresh(paths)?;
        Ok(JsonlRecorder {
            paths: paths.clone(),
            options,
            index: Mutex::new(index),
        })
    }

    pub fn paths(&self) -> &DatasetPaths {
        &self.paths
    }

    /// Every event on disk now, torn lines skipped and counted.
    pub fn read_all(&self) -> Result<ReadEvents> {
        store::read_all(&self.paths)
    }
}

impl Recorder for JsonlRecorder {
    fn append(&self, event: NewEvent) -> Result<Appended> {
        let file = self.paths.events_file(event.occurred_at.date_naive());
        let env = event.into_envelope()?;
        let mut index = self
            .index
            .lock()
            .map_err(|_| anyhow::anyhow!("event index lock poisoned"))?;
        let _guard = store::lock(&self.paths)?;
        // Other processes may have appended since this one last looked.
        index.refresh(&self.paths)?;
        if let Some(earlier) = index.get(&env.idempotency_key) {
            return Ok(Appended::Duplicate(earlier.clone()));
        }
        store::append(&self.paths, &file, &env)?;
        if self.options.abort_after_append {
            std::process::abort();
        }
        index.insert(env.clone());
        Ok(Appended::Recorded(env))
    }
}
