use crate::{Error, Result};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Version {
    pub id: String,
    pub version: u32,
}
impl Version {
    pub fn new(id: impl Into<String>, version: u32) -> Self {
        Self {
            id: id.into(),
            version,
        }
    }
    pub(crate) fn validate(&self) -> Result<()> {
        name(&self.id)?;
        if self.version == 0 {
            return Err(Error::Invalid("zero version"));
        }
        Ok(())
    }
}
pub(crate) fn name(s: &str) -> Result<()> {
    if s.is_empty()
        || s.len() > 128
        || !s
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
    {
        return Err(Error::Invalid(
            "identifier must be 1..128 ASCII letters, digits, dots, underscores or hyphens",
        ));
    }
    Ok(())
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum JsonShape {
    Text,
    Object,
    Integer,
    Boolean,
    NullOrInteger,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Schema {
    pub identity: Version,
    pub shape: JsonShape,
}
impl Schema {
    pub fn text() -> Self {
        Self {
            identity: Version::new("text", 1),
            shape: JsonShape::Text,
        }
    }
    pub(crate) fn accepts(&self, v: &Value) -> bool {
        match self.shape {
            JsonShape::Text => v.is_string(),
            JsonShape::Object => v.is_object(),
            JsonShape::Integer => v.is_i64(),
            JsonShape::Boolean => v.is_boolean(),
            JsonShape::NullOrInteger => v.is_null() || v.is_u64(),
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProcessingLimits {
    pub deliveries: usize,
    pub graphs: usize,
    pub bindings: usize,
    pub fanout: usize,
    pub input_bytes: usize,
    pub state_bytes: usize,
    pub output_bytes: usize,
    pub attempts: u32,
}
impl Default for ProcessingLimits {
    fn default() -> Self {
        Self {
            deliveries: 1024,
            graphs: 16,
            bindings: 64,
            fanout: 8,
            input_bytes: 4096,
            state_bytes: 4096,
            output_bytes: 16384,
            attempts: 3,
        }
    }
}
impl ProcessingLimits {
    pub(crate) fn validate(&self) -> Result<()> {
        if !(1..=65536).contains(&self.deliveries)
            || !(1..=64).contains(&self.graphs)
            || !(1..=1024).contains(&self.bindings)
            || !(1..=32).contains(&self.fanout)
            || !(64..=65536).contains(&self.input_bytes)
            || !(64..=65536).contains(&self.state_bytes)
            || !(256..=1048576).contains(&self.output_bytes)
            || !(1..=16).contains(&self.attempts)
        {
            return Err(Error::Invalid("processing limits out of range"));
        }
        Ok(())
    }
    /// Conservative logical allowance per permanently retained delivery, not physical disk bytes.
    pub fn reservation_bytes(&self) -> usize {
        self.input_bytes * 2 + self.state_bytes * 2 + self.output_bytes + self.fanout * 512 + 8192
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Scope {
    pub namespace: String,
    pub pipeline: String,
    pub node: String,
    pub key: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TerminalIdentity {
    pub namespace: String,
    pub client: String,
    pub conversation: String,
}
impl TerminalIdentity {
    pub fn local() -> Self {
        Self {
            namespace: "local".into(),
            client: "terminal".into(),
            conversation: "default".into(),
        }
    }
    pub(crate) fn validate(&self) -> Result<()> {
        name(&self.namespace)?;
        name(&self.client)?;
        name(&self.conversation)
    }
    pub(crate) fn key(&self) -> String {
        format!("{}:{}", self.client, self.conversation)
    }
}
/// A trusted-host capability. It cannot be decoded from event payloads.
#[derive(Debug, Clone)]
pub struct TerminalClient {
    pub(crate) store: String,
    pub(crate) identity: TerminalIdentity,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum NodeKind {
    Processor { routing: bool },
    Terminal { binding: Version },
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Node {
    pub code: Version,
    pub config_version: u32,
    pub config: Value,
    pub input: Schema,
    pub output: Schema,
    pub state_schema: Schema,
    pub initial_state: Value,
    pub kind: NodeKind,
    pub next: Vec<String>,
    pub branches: BTreeMap<String, Vec<String>>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Graph {
    pub namespace: String,
    pub pipeline: String,
    pub version: u32,
    pub entry: String,
    pub nodes: BTreeMap<String, Node>,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeliveryId(pub i64);
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProcessingEvent {
    pub envelope_version: u32,
    pub schema: Version,
    pub payload: Value,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Routing {
    ConfiguredNext,
    SelectedBranches(Vec<String>),
    Suppress(String),
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Proposal {
    pub reason: String,
    pub state: Value,
    pub outputs: Vec<Value>,
    pub routing: Routing,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CommittedOutcome {
    pub decision: Proposal,
    pub deliveries: Vec<DeliveryId>,
}
#[derive(Debug, Clone)]
pub struct ProcessingLease {
    pub(crate) store: String,
    pub(crate) incarnation: i64,
    pub(crate) generation: i64,
    pub(crate) worker: String,
    pub(crate) scope: Scope,
}
#[derive(Debug, Clone)]
pub struct ProcessingAttempt {
    pub(crate) id: DeliveryId,
    pub(crate) lease: ProcessingLease,
    pub(crate) attempt: u32,
    pub(crate) revision: i64,
    pub(crate) event: ProcessingEvent,
    pub(crate) state: Value,
    pub(crate) node: Node,
    pub(crate) graph: Graph,
    pub(crate) origin: TerminalIdentity,
    pub(crate) limits: ProcessingLimits,
}
impl ProcessingAttempt {
    pub fn delivery(&self) -> DeliveryId {
        self.id
    }
    pub fn scope(&self) -> &Scope {
        &self.lease.scope
    }
    pub fn event(&self) -> &ProcessingEvent {
        &self.event
    }
    pub fn state(&self) -> &Value {
        &self.state
    }
    pub fn config(&self) -> &Value {
        &self.node.config
    }
    pub fn limits(&self) -> &ProcessingLimits {
        &self.limits
    }
}
/// General executable graph component. Native implementations are trusted, not sandboxed.
pub trait Processor {
    fn code(&self) -> Version;
    fn process(&self, input: &ProcessingAttempt) -> Result<Proposal>;
}
/// Typed Rust adapter for a domain component. Scope and routes remain host controlled.
pub trait TypedProcessor {
    type Input: DeserializeOwned;
    type State: DeserializeOwned + Serialize;
    type Output: Serialize;
    fn code(&self) -> Version;
    fn process(
        &self,
        input: Self::Input,
        state: Self::State,
        config: &Value,
    ) -> Result<TypedProposal<Self::State, Self::Output>>;
}
pub struct TypedProposal<S, O> {
    pub reason: String,
    pub state: S,
    pub outputs: Vec<O>,
    pub routing: Routing,
}
pub struct RustAdapter<P>(pub P);
impl<P: TypedProcessor> Processor for RustAdapter<P> {
    fn code(&self) -> Version {
        self.0.code()
    }
    fn process(&self, a: &ProcessingAttempt) -> Result<Proposal> {
        let p = self.0.process(
            serde_json::from_value(a.event.payload.clone())?,
            serde_json::from_value(a.state.clone())?,
            &a.node.config,
        )?;
        Ok(Proposal {
            reason: p.reason,
            state: serde_json::to_value(p.state)?,
            outputs: p
                .outputs
                .into_iter()
                .map(serde_json::to_value)
                .collect::<std::result::Result<_, _>>()?,
            routing: p.routing,
        })
    }
}
#[derive(Debug, Clone)]
pub struct TerminalAttempt {
    pub(crate) id: DeliveryId,
    pub(crate) lease: ProcessingLease,
    pub(crate) payload: String,
    pub(crate) origin: TerminalIdentity,
}
impl TerminalAttempt {
    pub fn delivery(&self) -> DeliveryId {
        self.id
    }
    pub fn payload(&self) -> &str {
        &self.payload
    }
    pub fn identity(&self) -> &TerminalIdentity {
        &self.origin
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeliveryStatus {
    pub id: DeliveryId,
    pub scope: Scope,
    pub graph_version: u32,
    pub status: String,
    pub attempts: u32,
    pub failure: Option<String>,
    pub parent: Option<DeliveryId>,
    pub reason: Option<String>,
    pub children: Vec<DeliveryId>,
}
