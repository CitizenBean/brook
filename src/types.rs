use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Instant;

pub type Result<T> = std::result::Result<T, Error>;
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("store is already owned")]
    Locked,
    #[error("invalid or expired ownership token")]
    Fenced,
    #[error("operation key conflicts with its immutable submission")]
    Conflict,
    #[error("policy does not authorize this intent")]
    Unauthorized,
    #[error("request or job is closed, stale, or not ready")]
    NotReady,
    #[error("context revision changed; rebuild required")]
    RevisionChanged,
    #[error("configured capacity exceeded")]
    Capacity,
    #[error("required context cannot fit or is missing")]
    ContextUnavailable,
    #[error("invalid input: {0}")]
    Invalid(&'static str),
    #[error(transparent)]
    Harness(#[from] HarnessFailure),
    #[error(transparent)]
    Sql(#[from] rusqlite::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

/// Logical quotas, not a measured filesystem/RSS limit. Persisted at creation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Limits {
    pub sessions: i64,
    pub grants: i64,
    pub requests: i64,
    pub messages_per_session: i64,
    pub payload_bytes: usize,
    pub context_bytes: usize,
    pub job_attempts: i64,
    pub unused_retention_days: u32,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            sessions: 16,
            grants: 64,
            requests: 256,
            messages_per_session: 64,
            payload_bytes: 4096,
            context_bytes: 65536,
            job_attempts: 3,
            unused_retention_days: 7,
        }
    }
}
impl Limits {
    pub(crate) fn validate(&self) -> Result<()> {
        if !(1..=1024).contains(&self.sessions)
            || !(1..=4096).contains(&self.grants)
            || !(1..=65536).contains(&self.requests)
            || !(1..=1024).contains(&self.messages_per_session)
            || !(64..=65536).contains(&self.payload_bytes)
            || !(256..=1048576).contains(&self.context_bytes)
            || !(1..=16).contains(&self.job_attempts)
            || self.unused_retention_days == 0
        {
            return Err(Error::Invalid("limits outside supported bounds"));
        }
        Ok(())
    }
    /// Reserved logical bytes per request, including completion/control headroom.
    pub fn request_reservation(&self) -> i64 {
        (self.context_bytes + 32 * self.payload_bytes + 65536) as i64
    }
}

/// Elapsed milliseconds since this authority's clock was created. Must not regress.
pub trait Clock: Send + Sync {
    fn elapsed_ms(&self) -> i64;
}
pub struct MonotonicClock(Instant);
impl Default for MonotonicClock {
    fn default() -> Self {
        Self(Instant::now())
    }
}
impl Clock for MonotonicClock {
    fn elapsed_ms(&self) -> i64 {
        self.0.elapsed().as_millis().min(i64::MAX as u128) as i64
    }
}
#[derive(Default)]
pub struct ManualClock(AtomicI64);
impl ManualClock {
    pub fn set(&self, time: i64) {
        self.0.store(time, Ordering::SeqCst);
    }
}
impl Clock for ManualClock {
    fn elapsed_ms(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionId(pub i64);
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Receipt {
    pub request: i64,
}
#[derive(Debug, Clone)]
pub struct Lease {
    pub(crate) session: SessionId,
    pub(crate) worker: String,
    pub(crate) generation: i64,
    pub(crate) incarnation: i64,
    pub(crate) store_id: String,
}
impl Lease {
    pub fn session(&self) -> SessionId {
        self.session
    }
    pub fn generation(&self) -> i64 {
        self.generation
    }
}
/// Trusted host policy input. No natural-language authorization inference exists.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Destination {
    pub sink: String,
    pub account: String,
    pub recipient: String,
}
/// Agent settings are independent of the implementation that executes them.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentConfig {
    pub id: String,
    pub version: i64,
    pub role: String,
    pub instructions: String,
    pub model: String,
    pub temperature_milli: u16,
    pub max_output_tokens: u32,
    pub allowed_tools: Vec<String>,
}
impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            id: "assistant".into(),
            version: 1,
            role: "assistant".into(),
            instructions: "Process the correlated outcome.".into(),
            model: "deterministic".into(),
            temperature_milli: 0,
            max_output_tokens: 1024,
            allowed_tools: vec![],
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Submission {
    pub operation: String,
    pub agent: AgentConfig,
    pub grant: String,
    pub grant_version: i64,
    pub destination: Destination,
    pub instruction: String,
    pub payload: String,
    pub expected_peer: String,
    /// An ordered, duplicate-free list of immutable same-session event references.
    pub causal_events: Vec<i64>,
    /// Lifetime in active-authority milliseconds; downtime does not advance it.
    pub lifetime_ms: i64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    pub request: Option<i64>,
    pub id: i64,
    pub revision: i64,
    pub kind: String,
    pub text: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Context {
    pub agent: AgentConfig,
    pub session: SessionId,
    pub request: i64,
    pub revision: i64,
    pub cancellation: i64,
    pub instruction: String,
    pub payload: String,
    pub outcome: String,
    pub causal_events: Vec<i64>,
    pub history: Vec<Event>,
}
/// Immutable execution input; workers cannot alter the job identity or attempt fence.
#[derive(Debug, Clone)]
pub struct JobAttempt {
    pub(crate) request: i64,
    pub(crate) attempt: i64,
    pub(crate) lease: Lease,
    pub(crate) context: Context,
    pub(crate) output_budget: usize,
}
impl JobAttempt {
    pub fn output_budget(&self) -> usize {
        self.output_budget
    }
    pub fn context(&self) -> &Context {
        &self.context
    }
}
#[derive(Debug, Clone)]
pub struct SendAttempt {
    pub(crate) request: i64,
    pub(crate) lease: Lease,
    pub(crate) destination: Destination,
    pub(crate) payload: String,
}
impl SendAttempt {
    pub fn effect_id(&self) -> i64 {
        self.request
    }
    pub fn destination(&self) -> &Destination {
        &self.destination
    }
    pub fn payload(&self) -> &str {
        &self.payload
    }
}
#[derive(Debug, Clone, Copy)]
pub enum SendOutcome {
    Applied,
    ConfirmedNotApplied,
    Unknown,
}

/// Deterministic fault boundaries for the process-kill integration tests.
/// Hooks run synchronously; production uses the no-op default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Boundary {
    ProcessorIngressBeforeCommit,
    ProcessorIngressAfterCommit,
    ProcessorBeforeCommit,
    ProcessorAfterFirstChild,
    ProcessorAfterCommit,
    TerminalBeforeCommit,
    TerminalAfterCommit,
    TerminalWriteBeforeCommit,
    TerminalWriteAfterCommit,
    TerminalWritten,
    TerminalFinishBeforeCommit,
    TerminalFinishAfterCommit,
    AdmissionBeforeCommit,
    AdmissionAfterCommit,
    ReplyBeforeCommit,
    ReplyAfterCommit,
    SendBeforeCommit,
    SendAfterCommit,
    JobBeforeCommit,
    JobAfterCommit,
    ResultBeforeCommit,
    ResultAfterCommit,
    QuarantineBeforeCommit,
    QuarantineAfterCommit,
    RecoveryBeforeCommit,
    RecoveryAfterCommit,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn limits_reject_unbounded_or_zero_settings() {
        let defaults = Limits::default();
        assert!(defaults.validate().is_ok());
        for limits in [
            Limits {
                requests: 0,
                ..defaults.clone()
            },
            Limits {
                context_bytes: usize::MAX,
                ..defaults.clone()
            },
            Limits {
                job_attempts: 0,
                ..defaults.clone()
            },
            Limits {
                unused_retention_days: 0,
                ..defaults
            },
        ] {
            assert!(limits.validate().is_err());
        }
    }
}

/// Bounded failure classification, persisted through Store::fail_job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum HarnessFailure {
    #[error("harness result cannot fit its byte budget")]
    OutputBudgetExceeded,
    #[error("harness execution failed")]
    ExecutionFailed,
}
