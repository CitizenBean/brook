//! Host test driver: composes public APIs; this is not a production graph/agent bridge.
use brook::{processing::*, *};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error as StdError,
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
};
type TestResult<T> = std::result::Result<T, Box<dyn StdError>>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Action {
    Admit(usize),
    Duplicate(usize),
    Send(usize),
    Revoke(usize),
    Cancel(usize),
    Reply(usize),
    DuplicateReply(usize),
    Activity(usize),
    Resume {
        work: usize,
        repeat_physical: bool,
        fail: bool,
    },
    Reopen,
    ExpireOwner,
    Publish(usize),
    Print {
        work: usize,
        uncertain: bool,
    },
    Advance(u32),
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Plan {
    pub version: u32,
    pub seed: u64,
    pub actions: Vec<Action>,
}
/// Fixed algorithm; the serialized action list, not only the seed, is the replay contract.
pub fn plan(seed: u64) -> Plan {
    let mut rng = seed.wrapping_add(0x9e3779b97f4a7c15);
    let mut next = || {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        rng
    };
    let mut actions = Vec::new();
    for w in 0..6 {
        actions.push(Action::Admit(w));
        actions.push(Action::Duplicate(w));
        if w == 5 {
            actions.push(Action::Revoke(w));
        }
        actions.push(Action::Send(w));
    }
    actions.extend([
        Action::Cancel(2),
        Action::Activity(0),
        Action::Activity(1),
        Action::Reopen,
        Action::ExpireOwner,
    ]);
    let mut replies = vec![1, 3, 4];
    for i in (1..replies.len()).rev() {
        let j = (next() as usize) % (i + 1);
        replies.swap(i, j);
    }
    replies.extend([0, 2, 5]);
    for w in replies {
        actions.push(Action::Advance((next() % 7) as u32));
        actions.push(Action::Reply(w));
        if ![2, 5].contains(&w) {
            actions.push(Action::DuplicateReply(w));
            actions.push(Action::Resume {
                work: w,
                repeat_physical: w == 0,
                fail: w == 3,
            });
            if w == 1 {
                actions.extend([Action::Activity(0), Action::Reopen]);
            }
        }
    }
    actions.push(Action::Reopen);
    for w in [4, 1, 0] {
        actions.push(Action::Publish(w));
        actions.push(Action::Print {
            work: w,
            uncertain: w == 4,
        });
    }
    Plan {
        version: 2,
        seed,
        actions,
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Invocation {
    pub context: Context,
    pub harness: String,
    pub request: i64,
    pub result: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Effect {
    pub delivery: DeliveryId,
    pub text: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolEffect {
    pub request: i64,
    pub destination: Destination,
    pub payload: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Work {
    pub root: DeliveryId,
    pub session: SessionId,
    pub client: TerminalIdentity,
    pub input: Submission,
    pub receipt: Receipt,
    pub delivery: DeliveryId,
    pub output: Option<DeliveryId>,
    pub expected_result: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestView {
    pub id: i64,
    pub session: SessionId,
    pub state: String,
    pub outbox: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub clock_epoch: u32,
    pub elapsed_ms: i64,
    pub tool_effects: BTreeMap<i64, ToolEffect>,
    pub pending_deliveries: Vec<DeliveryId>,
    pub histories: BTreeMap<i64, Vec<Event>>,
    pub requests: Vec<RequestView>,
    pub deliveries: Vec<DeliveryStatus>,
    pub effects: Vec<Effect>,
    pub invocations: Vec<Invocation>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Frame {
    pub observation_failure: Option<String>,
    pub action: Action,
    pub observed: Snapshot,
    pub assertions: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trace {
    pub reopen_observations: Vec<ReopenObservation>,
    pub format: String,
    pub plan: Plan,
    pub graphs: Vec<Graph>,
    pub limits: Limits,
    pub processing_limits: ProcessingLimits,
    pub works: Vec<Work>,
    pub frames: Vec<Frame>,
    pub failure: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReopenObservation {
    pub before: Snapshot,
    pub after: Snapshot,
}

fn require(ok: bool, message: &str) -> TestResult<()> {
    if !ok {
        return Err(message.into());
    }
    Ok(())
}
/// Fixture-policy oracle: checks the complete observed graph, in both directions.
/// Expected edges come from the scenario contract, not committed child contents.
fn verify_graph(snapshot: &Snapshot, works: &[Work]) -> TestResult<()> {
    let records: BTreeMap<_, _> = snapshot.deliveries.iter().map(|d| (d.id.0, d)).collect();
    require(
        records.len() == snapshot.deliveries.len(),
        "duplicate delivery record identity",
    )?;
    for d in &snapshot.deliveries {
        require(
            d.graph_version == 1 && d.scope.pipeline == "terminal",
            "child graph version or pipeline changed",
        )?;
        let next = match d.scope.node.as_str() {
            "filter" => Some("route"),
            "route" => Some("uppercase"),
            "uppercase" => Some("work"),
            "work" => Some("output"),
            "output" => None,
            _ => return Err("child reached unexpected node".into()),
        };
        if let Some(parent) = d.parent {
            let parent = records
                .get(&parent.0)
                .ok_or("child parent record missing")?;
            require(
                parent.children.contains(&d.id),
                "child absent from parent commitment",
            )?;
            require(
                parent.scope.namespace == d.scope.namespace
                    && parent.scope.pipeline == d.scope.pipeline
                    && parent.scope.key == d.scope.key
                    && parent.graph_version == d.graph_version,
                "child crossed parent scope or graph",
            )?;
        } else {
            let work = works
                .iter()
                .find(|w| w.root == d.id)
                .ok_or("unbound root delivery")?;
            require(
                d.scope.node == "filter"
                    && d.scope.namespace == work.client.namespace
                    && d.scope.key
                        == format!("{}:{}", work.client.client, work.client.conversation),
                "root identity disagrees with ingress fixture",
            )?;
        }
        if d.status == "done" {
            require(d.children.len() == 1, "missing configured child")?;
            let child = records
                .get(&d.children[0].0)
                .ok_or("committed child record missing")?;
            require(child.parent == Some(d.id), "child parent identity mismatch")?;
            require(
                next == Some(child.scope.node.as_str()),
                "child reached unexpected node",
            )?;
            require(
                child.scope.namespace == d.scope.namespace
                    && child.scope.key == d.scope.key
                    && child.scope.pipeline == d.scope.pipeline
                    && child.graph_version == d.graph_version,
                "child crossed parent scope or graph",
            )?;
        } else {
            require(
                d.children.is_empty(),
                "uncommitted or terminal delivery has children",
            )?;
        }
    }
    let expected_pending: BTreeSet<_> = snapshot
        .deliveries
        .iter()
        .filter(|d| d.status == "pending")
        .map(|d| d.id.0)
        .collect();
    let actual_pending: BTreeSet<_> = snapshot.pending_deliveries.iter().map(|d| d.0).collect();
    require(
        actual_pending.len() == snapshot.pending_deliveries.len()
            && actual_pending == expected_pending,
        "complete pending set differs from fixture closure",
    )?;
    for (index, w) in works.iter().enumerate() {
        if let Some(effect) = snapshot.tool_effects.get(&w.receipt.request) {
            let expected = Destination {
                sink: "fake".into(),
                account: format!("account-{}", index / 3),
                recipient: format!("peer-{}", index / 3),
            };
            require(index != 5, "revoked tool effect happened externally")?;
            require(
                effect.request == w.receipt.request,
                "tool effect request identity mismatch",
            )?;
            require(
                effect.destination == expected,
                "external tool destination violated fixture authorization",
            )?;
            require(
                effect.payload == format!("TASK-{index}"),
                "external tool payload violated fixture authorization",
            )?;
        }
    }
    require(
        snapshot
            .tool_effects
            .keys()
            .all(|id| works.iter().any(|w| w.receipt.request == *id)),
        "unbound external tool effect",
    )?;
    Ok(())
}

/// Independent event-log oracle. It does not reproduce Store's SQL or state transitions.
pub fn verify(trace: &Trace, complete: bool) -> TestResult<()> {
    for reopen in &trace.reopen_observations {
        verify_graph(&reopen.before, &trace.works)?;
        verify_graph(&reopen.after, &trace.works)?;
        require(
            reopen.before.histories == reopen.after.histories,
            "reopen rewrote durable history",
        )?;
        require(
            reopen.before.tool_effects == reopen.after.tool_effects,
            "reopen changed external tool effects",
        )?;
        for old in &reopen.before.deliveries {
            let new = reopen
                .after
                .deliveries
                .iter()
                .find(|d| d.id == old.id)
                .ok_or("reopen lost delivery")?;
            require(old.attempts == new.attempts, "reopen reset attempt budget")?;
        }
    }
    let mut previous: BTreeMap<i64, Vec<Event>> = BTreeMap::new();
    let owners: BTreeMap<i64, i64> = trace
        .works
        .iter()
        .map(|w| (w.receipt.request, w.session.0))
        .collect();
    let mut attempts = BTreeMap::new();
    for frame in &trace.frames {
        verify_graph(&frame.observed, &trace.works)?;
        for (sid, history) in &frame.observed.histories {
            if let Some(old) = previous.get(sid) {
                require(history.starts_with(old), "history rewound or changed")?;
            }
            let mut unique = BTreeSet::new();
            for e in history {
                if let Some(r) = e.request {
                    require(
                        owners.get(&r) == Some(sid),
                        "request event crossed session boundary",
                    )?;
                    require(
                        unique.insert((r, e.kind.clone())),
                        "duplicate logical event",
                    )?;
                }
            }
            previous.insert(*sid, history.clone());
        }
        let mut effects = BTreeSet::new();
        for e in &frame.observed.effects {
            require(
                effects.insert(e.delivery.0),
                "physical terminal effect repeated",
            )?;
        }
        for pending in &frame.observed.pending_deliveries {
            require(
                frame.observed.deliveries.iter().any(|d| d.id == *pending),
                "unaccounted pending delivery",
            )?;
        }
        for invocation in &frame.observed.invocations {
            let owner = owners
                .get(&invocation.request)
                .ok_or("invocation has no admitted owner")?;
            let (index, work) = trace
                .works
                .iter()
                .enumerate()
                .find(|(_, w)| w.receipt.request == invocation.request)
                .ok_or("invocation fixture missing")?;
            let c = &invocation.context;
            require(
                c.request == invocation.request,
                "context request identity mismatch",
            )?;
            require(
                c.agent == work.input.agent
                    && c.instruction == work.input.instruction
                    && c.payload == work.input.payload
                    && c.causal_events == work.input.causal_events,
                "immutable context input differs from admission",
            )?;
            require(
                c.outcome == format!("result-{index}"),
                "context reply correlation mismatch",
            )?;
            require(
                invocation.context.session.0 == *owner,
                "cross-session context",
            )?;
            require(
                frame.observed.histories.get(owner).is_some_and(|history| {
                    invocation
                        .context
                        .history
                        .iter()
                        .all(|e| history.contains(e))
                }),
                "cross-session context history or fabricated evidence",
            )?;
            for e in &invocation.context.history {
                if let Some(r) = e.request {
                    require(
                        owners.get(&r) == Some(owner),
                        "cross-session context history",
                    )?;
                }
            }
        }
        for d in &frame.observed.deliveries {
            if let Some(previous) = attempts.insert(d.id.0, d.attempts) {
                require(d.attempts >= previous, "physical attempt budget reset")?;
            }
            if d.status == "done" {
                require(d.children.len() == 1, "missing configured child")?;
            }

            require(
                d.children
                    .iter()
                    .map(|d| d.0)
                    .collect::<BTreeSet<_>>()
                    .len()
                    == d.children.len(),
                "duplicate child identity",
            )?;
        }
        for work in &trace.works {
            let Some(view) = frame
                .observed
                .requests
                .iter()
                .find(|r| r.id == work.receipt.request)
            else {
                continue;
            };
            let delivery = frame
                .observed
                .deliveries
                .iter()
                .find(|d| d.id == work.delivery)
                .ok_or("admitted work delivery record missing")?;
            require(
                delivery.scope.node == "work",
                "work delivery identity mismatch",
            )?;
            if delivery.status == "done" {
                let output = work.output.ok_or("completed work has no output identity")?;
                require(
                    delivery.children == vec![output],
                    "work output identity differs from committed child",
                )?;
                require(
                    frame.observed.deliveries.iter().any(|d| d.id == output),
                    "work output row missing",
                )?;
            }
            let history = &frame.observed.histories[&work.session.0];
            let results: Vec<_> = history
                .iter()
                .filter(|e| e.request == Some(view.id) && e.kind == "result")
                .collect();
            require(results.len() <= 1, "logical result duplicated")?;
            if view.state == "done" {
                require(results.len() == 1, "done request has no committed result")?;
                require(
                    results[0].text == work.expected_result,
                    "result disagrees with independent script expectation",
                )?;
            }
            if matches!(view.state.as_str(), "cancelled" | "recovery_failed") {
                require(
                    results.is_empty(),
                    "closed or failed work acquired a result",
                )?;
            }
            if let Some(output) = work.output {
                if let Some(d) = frame.observed.deliveries.iter().find(|d| d.id == output) {
                    require(
                        d.scope.namespace == work.client.namespace
                            && d.scope.key
                                == format!("{}:{}", work.client.client, work.client.conversation),
                        "output crossed authorized scope",
                    )?;
                }
            }
        }
    }
    if complete {
        require(trace.works.len() == 6, "incomplete work set")?;
        require(trace.failure.is_none(), "trace records a failed action")?;
        require(
            trace.frames.len() == trace.plan.actions.len(),
            "incomplete action trace",
        )?;
        let last = &trace.frames.last().ok_or("empty trace")?.observed;
        let expected = [
            "done",
            "done",
            "cancelled",
            "recovery_failed",
            "done",
            "cancelled",
        ];
        for (w, state) in trace.works.iter().zip(expected) {
            require(
                last.requests
                    .iter()
                    .find(|r| r.id == w.receipt.request)
                    .unwrap()
                    .state
                    == state,
                "final request state differs from script",
            )?;
        }
        require(last.effects.len() == 3, "missing terminal effects")?;
        require(
            last.tool_effects.len() == 5,
            "external tool journal disagrees with admissions",
        )?;
        require(
            !last
                .tool_effects
                .contains_key(&trace.works[5].receipt.request),
            "revoked tool effect happened externally",
        )?;
        for w in [0, 1, 4] {
            let work = &trace.works[w];
            let d = work.output.ok_or("missing correlated terminal output")?;
            let effect = last
                .effects
                .iter()
                .find(|e| e.delivery == d)
                .ok_or("missing authorized effect")?;
            require(
                effect.text == format!("{}\n", work.expected_result),
                "sink output differs from independent expectation",
            )?;
        }
        let count = last
            .invocations
            .iter()
            .filter(|i| i.request == trace.works[0].receipt.request)
            .count();
        require(count == 2, "repeat-physical scenario did not invoke twice")?;
        require(
            last.requests
                .iter()
                .find(|r| r.id == trace.works[4].receipt.request)
                .unwrap()
                .outbox
                == "unknown",
            "unknown async effect was cleared by reply",
        )?;
        let output = trace.works[4].output.unwrap();
        require(
            last.deliveries
                .iter()
                .find(|d| d.id == output)
                .unwrap()
                .status
                == "unknown",
            "unknown terminal effect was retried or cleared",
        )?;
    }
    Ok(())
}

/// Fake reasoning only. The host driver, never this Harness, admits tool requests.
pub struct ScriptedHarness {
    pub fail: bool,
    pub expected_request: i64,
    pub required_history: String,
}
impl Harness for ScriptedHarness {
    fn name(&self) -> &'static str {
        "scripted-simulation-v1"
    }
    fn run(&self, c: &Context, budget: usize) -> std::result::Result<String, HarnessFailure> {
        if self.fail
            || c.request != self.expected_request
            || !c.history.iter().any(|e| e.text == self.required_history)
        {
            return Err(HarnessFailure::ExecutionFailed);
        }
        let answer = format!("resolved {}: {}", c.payload, c.outcome);
        if answer.len() > budget {
            return Err(HarnessFailure::OutputBudgetExceeded);
        }
        Ok(answer)
    }
}

/// Pure test processor: the host supplies a committed result, not a tool capability.
pub struct ResultProcessor {
    pub delivery: DeliveryId,
    pub result: String,
}
impl Processor for ResultProcessor {
    fn code(&self) -> Version {
        Version::new("host-result", 1)
    }
    fn process(&self, a: &ProcessingAttempt) -> brook::Result<Proposal> {
        if a.delivery() != self.delivery {
            return Err(Error::Invalid("host result correlation mismatch"));
        }
        Ok(Proposal {
            reason: "host test driver forwards committed correlated result".into(),
            state: json!(1),
            outputs: vec![json!(self.result)],
            routing: Routing::ConfiguredNext,
        })
    }
}

struct EffectWriter {
    file: std::fs::File,
    fail_flush: bool,
}
impl Write for EffectWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.file.write(bytes)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()?;
        self.file.sync_all()?;
        if self.fail_flush {
            Err(std::io::Error::other(
                "injected flush uncertainty after durable external write",
            ))
        } else {
            Ok(())
        }
    }
}

pub struct Driver {
    path: PathBuf,
    pub store: Store,
    clock: Arc<ManualClock>,
    elapsed: i64,
    epoch: u32,
    delivery_ids: Vec<DeliveryId>,
    reopen_observations: Vec<ReopenObservation>,
    sessions: Vec<SessionId>,
    leases: Vec<Lease>,
    clients: Vec<TerminalClient>,
    pub works: Vec<Work>,
    effects: Vec<Effect>,
    invocations: Vec<Invocation>,
}
impl Driver {
    pub fn new(path: &Path) -> TestResult<Self> {
        require(
            !path.join("brook.sqlite3").exists(),
            "host simulation requires a fresh store directory",
        )?;
        let clock = Arc::new(ManualClock::default());
        let mut store = Store::open(path, Limits::default(), clock.clone())?;
        store.configure_processing(Default::default())?;
        let mut sessions = Vec::new();
        let mut leases = Vec::new();
        let mut clients = Vec::new();
        for n in 0..2 {
            let identity = TerminalIdentity {
                namespace: format!("namespace-{n}"),
                client: "terminal".into(),
                conversation: "same-external-id".into(),
            };
            let client = store.terminal_client(identity.clone())?;
            let binding = Version::new(format!("output-{n}"), 1);
            store.bind_terminal(&binding, &client)?;
            let mut g = terminal_recipe(&identity, binding)?;
            insert_uppercase(&mut g)?;
            let mut work = g.nodes["uppercase"].clone();
            work.code = Version::new("host-result", 1);
            g.nodes.insert("work".into(), work);
            g.nodes.get_mut("uppercase").unwrap().next = vec!["work".into()];
            store.register_graph(&g)?;
            let sid = store.resolve_session(
                &identity.namespace,
                &identity.client,
                &identity.conversation,
            )?;
            sessions.push(sid);
            leases.push(store.claim(sid, "host", 10000)?);
            clients.push(client);
        }
        require(
            sessions[0] != sessions[1],
            "namespace mapping collapsed identical external IDs",
        )?;
        Ok(Self {
            path: path.into(),
            store,
            clock,
            elapsed: 0,
            epoch: 0,
            delivery_ids: vec![],
            reopen_observations: vec![],
            sessions,
            leases,
            clients,
            works: vec![],
            effects: vec![],
            invocations: vec![],
        })
    }
    pub fn reopen(&mut self) -> TestResult<()> {
        let before = self.snapshot()?;
        // Release the actual Store/OS lock before reopening, without a simulated DB.
        let placeholder = tempfile::tempdir()?;
        let dummy = Store::open(
            placeholder.path(),
            Limits::default(),
            Arc::new(ManualClock::default()),
        )?;
        drop(std::mem::replace(&mut self.store, dummy));
        self.clock = Arc::new(ManualClock::default());
        self.elapsed = 0;
        self.epoch += 1;
        self.store = Store::open(&self.path, Limits::default(), self.clock.clone())?;
        self.leases.clear();
        for sid in &self.sessions {
            self.leases
                .push(self.store.claim(*sid, "host-restarted", 10000)?);
        }
        let after = self.snapshot()?;
        require(
            before.histories == after.histories,
            "restart changed history",
        )?;
        self.reopen_observations
            .push(ReopenObservation { before, after });
        Ok(())
    }
    fn graph_step(&mut self, id: DeliveryId) -> TestResult<Vec<DeliveryId>> {
        let view = self.store.inspect_processing(id)?;
        let code = match view.scope.node.as_str() {
            "filter" => "filter",
            "route" => "router",
            "uppercase" => "uppercase",
            _ => return Err("unexpected prefix node".into()),
        };
        let lease = self.store.claim_processing(id, "graph", 10000)?;
        let a = self.store.prepare_processing(&lease, id)?;
        let p = self
            .store
            .execute_processor(&a, &RustAdapter(TextProcessor(code)))?;
        let done = self.store.commit_processing(&a, &p)?;
        require(
            self.store.commit_processing(&a, &p)? == done,
            "duplicate processor completion changed outcome",
        )?;
        self.store.release_processing(&lease)?;
        self.delivery_ids.extend(done.deliveries.iter().copied());
        Ok(done.deliveries)
    }
    fn admit(&mut self, n: usize) -> TestResult<()> {
        require(
            n == self.works.len(),
            "admissions must use contiguous script IDs",
        )?;
        let actor = n / 3;
        let text = format!("task-{n}");
        let mut id = self.store.submit_terminal(
            &self.clients[actor],
            "terminal",
            1,
            &format!("input-{n}"),
            json!(text),
        )?;
        let root = id;
        self.delivery_ids.push(id);
        for _ in 0..3 {
            let children = self.graph_step(id)?;
            require(
                children.len() == 1,
                "prefix must have exactly one configured child",
            )?;
            id = children[0];
        }
        let lease = self.store.claim_processing(id, "inspect-work", 10000)?;
        let a = self.store.prepare_processing(&lease, id)?;
        let payload = a
            .event()
            .payload
            .as_str()
            .ok_or("work input is not text")?
            .to_owned();
        require(
            payload == format!("TASK-{n}"),
            "configured transform absent",
        )?;
        self.store
            .fail_processing(&a, "host test driver awaiting async request", true)?;
        self.store.release_processing(&lease)?;
        let causal = self
            .store
            .message(self.sessions[actor], &format!("source-{n}"))?;
        let destination = Destination {
            sink: "fake".into(),
            account: format!("account-{actor}"),
            recipient: format!("peer-{actor}"),
        };
        let grant = format!("grant-{n}");
        let version = self.store.grant(
            &grant,
            self.sessions[actor],
            &destination,
            &payload,
            1000000,
        )?;
        let input = Submission {
            operation: format!("host-delivery-{}", id.0),
            agent: AgentConfig::default(),
            grant,
            grant_version: version,
            destination,
            instruction: format!("host-script-tool-{n}"),
            payload,
            expected_peer: format!("peer-{actor}"),
            causal_events: vec![causal],
            lifetime_ms: 1000000,
        };
        let receipt = self.store.admit(&self.leases[actor], &input)?;
        self.works.push(Work {
            root,
            session: self.sessions[actor],
            client: TerminalIdentity {
                namespace: format!("namespace-{actor}"),
                client: "terminal".into(),
                conversation: "same-external-id".into(),
            },
            expected_result: if n == 0 {
                format!("request {}: result-0", receipt.request)
            } else {
                format!("resolved TASK-{n}: result-{n}")
            },
            input,
            receipt,
            delivery: id,
            output: None,
        });
        Ok(())
    }
    pub fn action(&mut self, action: &Action) -> TestResult<Vec<String>> {
        let mut checks = vec![];
        match *action {
            Action::Activity(n) => require(n < self.sessions.len(), "unknown session script ID")?,
            Action::Admit(n) => require(
                n < 6 && n == self.works.len(),
                "invalid admission script ID",
            )?,
            Action::Duplicate(n)
            | Action::Send(n)
            | Action::Revoke(n)
            | Action::Cancel(n)
            | Action::Reply(n)
            | Action::DuplicateReply(n)
            | Action::Publish(n)
            | Action::Resume { work: n, .. }
            | Action::Print { work: n, .. } => {
                require(n < self.works.len(), "unknown work script ID")?
            }
            Action::ExpireOwner => {
                require(!self.works.is_empty(), "owner expiry before admission")?
            }
            _ => {}
        }
        match *action {
            Action::Admit(n) => {
                self.admit(n)?;
                checks.push("configured transform and host correlation".into());
            }
            Action::Duplicate(n) => {
                let w = &self.works[n];
                require(
                    self.store.admit(&self.leases[n / 3], &w.input)? == w.receipt,
                    "duplicate changed receipt",
                )?;
                let mut conflict = w.input.clone();
                conflict.payload.push('x');
                require(
                    matches!(
                        self.store.admit(&self.leases[n / 3], &conflict),
                        Err(Error::Conflict)
                    ),
                    "conflicting intent accepted",
                )?;
                checks.push("matching and conflicting request retries".into());
            }
            Action::Revoke(n) => self.store.revoke(&self.works[n].input.grant)?,
            Action::Send(n) => {
                let w = &self.works[n];
                if n == 5 {
                    require(
                        matches!(
                            self.store.begin_send(&self.leases[n / 3], w.receipt),
                            Err(Error::Unauthorized)
                        ),
                        "revoked grant admitted an effect",
                    )?;
                    self.store.cancel_request(&self.leases[n / 3], w.receipt)?;
                } else {
                    let a = self.store.begin_send(&self.leases[n / 3], w.receipt)?;
                    let mut journal = std::fs::OpenOptions::new()
                        .create_new(true)
                        .write(true)
                        .open(self.path.join(format!("tool-effect-{}", w.receipt.request)))?;
                    journal.write_all(&serde_json::to_vec(&ToolEffect {
                        request: a.effect_id(),
                        destination: a.destination().clone(),
                        payload: a.payload().to_owned(),
                    })?)?;
                    journal.sync_all()?;
                    if n != 4 {
                        let sink = FakeSink {
                            outcome: SendOutcome::Applied,
                        };
                        self.store.finish_send(&a, sink.send(&a))?;
                    }
                }
            }
            Action::Cancel(n) => self
                .store
                .cancel_request(&self.leases[n / 3], self.works[n].receipt)?,
            Action::Activity(n) => {
                self.store
                    .message(self.sessions[n], &format!("newer-session-{n}"))?;
            }
            Action::Advance(ms) => {
                self.elapsed += i64::from(ms);
                self.clock.set(self.elapsed);
            }
            Action::Reopen => self.reopen()?,
            Action::ExpireOwner => {
                let stale = self.leases[0].clone();
                self.elapsed += 10001;
                self.clock.set(self.elapsed);
                for n in 0..2 {
                    self.leases[n] = self.store.claim(self.sessions[n], "takeover", 10000)?;
                }
                require(
                    matches!(
                        self.store.admit(&stale, &self.works[0].input),
                        Err(Error::Fenced)
                    ),
                    "stale owner accepted",
                )?;
                checks.push("expired owner fenced after takeover".into());
            }
            Action::Reply(n) => {
                let w = &self.works[n];
                let text = format!("result-{n}");
                if [2, 5].contains(&n) {
                    require(
                        self.store
                            .accept_reply(w.receipt, &w.input.expected_peer, &text)
                            .is_err(),
                        "cancelled request resumed",
                    )?;
                } else {
                    require(
                        matches!(
                            self.store.accept_reply(w.receipt, "wrong-peer", &text),
                            Err(Error::Unauthorized)
                        ),
                        "wrong peer accepted",
                    )?;
                    require(
                        self.store
                            .accept_reply(w.receipt, &w.input.expected_peer, &text)?,
                        "first reply was not admitted",
                    )?;
                }
            }
            Action::DuplicateReply(n) => {
                let w = &self.works[n];
                require(
                    !self.store.accept_reply(
                        w.receipt,
                        &w.input.expected_peer,
                        &format!("result-{n}"),
                    )?,
                    "duplicate reply accepted twice",
                )?;
                require(
                    matches!(
                        self.store
                            .accept_reply(w.receipt, &w.input.expected_peer, "conflict"),
                        Err(Error::Conflict)
                    ),
                    "conflicting reply accepted",
                )?;
            }
            Action::Resume {
                work: n,
                repeat_physical,
                fail,
            } => {
                let receipt = self.works[n].receipt;
                let context = self.store.build_context(receipt)?;
                require(
                    context.history == self.store.history(self.works[n].session)?,
                    "resume did not read the current complete history",
                )?;
                require(
                    context
                        .history
                        .iter()
                        .any(|e| e.text == format!("newer-session-{}", n / 3)),
                    "later context missing",
                )?;
                require(
                    !context
                        .history
                        .iter()
                        .any(|e| e.text == format!("newer-session-{}", 1 - n / 3)),
                    "other session leaked into context",
                )?;
                self.store.admit_resume(&self.leases[n / 3], &context)?;
                let mut a = self.store.claim_job(&self.leases[n / 3], receipt)?;
                let harness = ScriptedHarness {
                    fail,
                    expected_request: receipt.request,
                    required_history: format!("newer-session-{}", n / 3),
                };
                let mut output = harness.run(a.context(), a.output_budget());
                self.invocations.push(Invocation {
                    context: a.context().clone(),
                    harness: harness.name().into(),
                    request: receipt.request,
                    result: output.as_ref().ok().cloned(),
                });
                if repeat_physical {
                    self.reopen()?;
                    require(
                        matches!(
                            self.store.complete_job(&a, "stale result"),
                            Err(Error::Fenced)
                        ),
                        "stale physical result committed",
                    )?;
                    a = self.store.claim_job(&self.leases[n / 3], receipt)?;
                    output = EchoHarness.run(a.context(), a.output_budget());
                    self.invocations.push(Invocation {
                        context: a.context().clone(),
                        harness: EchoHarness.name().into(),
                        request: receipt.request,
                        result: output.as_ref().ok().cloned(),
                    });
                }
                match output {
                    Ok(text) => {
                        self.store.complete_job(&a, &text)?;
                        require(
                            self.store.complete_job(&a, &text).is_err(),
                            "logical completion repeated",
                        )?;
                    }
                    Err(e) => self.store.fail_job(&a, e)?,
                }
                checks.push(
                    "current context, separate physical invocations and logical completion".into(),
                );
            }
            Action::Publish(n) => {
                let w = &self.works[n];
                let history = self.store.history(w.session)?;
                let result = history
                    .iter()
                    .find(|e| e.kind == "result" && e.request == Some(w.receipt.request))
                    .ok_or("cannot publish uncommitted result")?
                    .text
                    .clone();
                let lease = self
                    .store
                    .claim_processing(w.delivery, "host-publish", 10000)?;
                let a = self.store.prepare_processing(&lease, w.delivery)?;
                let proposal = self.store.execute_processor(
                    &a,
                    &ResultProcessor {
                        delivery: w.delivery,
                        result,
                    },
                )?;
                let done = self.store.commit_processing(&a, &proposal)?;
                require(done.deliveries.len() == 1, "unexpected result fanout")?;
                self.store.release_processing(&lease)?;
                self.delivery_ids.extend(done.deliveries.iter().copied());
                self.works[n].output = Some(done.deliveries[0]);
            }
            Action::Print { work: n, uncertain } => {
                let id = self.works[n].output.ok_or("output missing")?;
                let lease = self.store.claim_processing(id, "sink", 10000)?;
                let a = self
                    .store
                    .begin_terminal(&lease, id, &self.clients[n / 3])?;
                let journal_path = self.path.join(format!("terminal-effect-{}", id.0));
                let mut writer = EffectWriter {
                    file: std::fs::OpenOptions::new()
                        .create_new(true)
                        .write(true)
                        .open(&journal_path)?,
                    fail_flush: uncertain,
                };
                if uncertain {
                    require(
                        matches!(
                            self.store.print_terminal(&a, &mut writer),
                            Err(Error::Io(_))
                        ),
                        "flush fault did not report uncertainty",
                    )?;
                    let before = std::fs::metadata(&journal_path)?.len();
                    require(
                        matches!(
                            self.store.print_terminal(&a.clone(), &mut writer),
                            Err(Error::NotReady)
                        ),
                        "uncertain output repeated",
                    )?;
                    require(
                        std::fs::metadata(&journal_path)?.len() == before,
                        "second output escaped admission guard",
                    )?;
                } else {
                    self.store.print_terminal(&a, &mut writer)?;
                    self.store.release_processing(&lease)?;
                }
                self.effects.push(Effect {
                    delivery: id,
                    text: std::fs::read_to_string(&journal_path)?,
                });
                if uncertain {
                    self.reopen()?;
                }
            }
        }
        Ok(checks)
    }
    pub fn snapshot(&self) -> TestResult<Snapshot> {
        let mut histories = BTreeMap::new();
        for sid in &self.sessions {
            histories.insert(sid.0, self.store.history(*sid)?);
        }
        let mut requests = vec![];
        let mut deliveries = vec![];
        for id in &self.delivery_ids {
            deliveries.push(self.store.inspect_processing(*id)?);
        }
        let mut tool_effects = BTreeMap::new();
        for w in &self.works {
            requests.push(RequestView {
                id: w.receipt.request,
                session: w.session,
                state: self.store.request_state(w.receipt)?,
                outbox: self.store.outbox_state(w.receipt)?,
            });
            let path = self.path.join(format!("tool-effect-{}", w.receipt.request));
            if path.exists() {
                tool_effects.insert(
                    w.receipt.request,
                    serde_json::from_slice(&std::fs::read(path)?)?,
                );
            }
        }
        // Journal bytes are read independently of Brook's effect state on every observation.
        let effects = self
            .effects
            .iter()
            .map(|e| {
                Ok(Effect {
                    delivery: e.delivery,
                    text: std::fs::read_to_string(
                        self.path.join(format!("terminal-effect-{}", e.delivery.0)),
                    )?,
                })
            })
            .collect::<TestResult<Vec<_>>>()?;
        Ok(Snapshot {
            clock_epoch: self.epoch,
            elapsed_ms: self.elapsed,
            tool_effects,
            pending_deliveries: self.store.pending_processing(64)?,
            histories,
            requests,
            deliveries,
            effects,
            invocations: self.invocations.clone(),
        })
    }
}
pub fn run(path: &Path, plan: Plan, trace_path: Option<&Path>) -> TestResult<Trace> {
    run_with_observer(path, plan, trace_path, Driver::snapshot)
}
/// Test seam for observation failure; actions still execute on the actual Store.
pub fn run_with_observer(
    path: &Path,
    plan: Plan,
    trace_path: Option<&Path>,
    mut observe: impl FnMut(&Driver) -> TestResult<Snapshot>,
) -> TestResult<Trace> {
    require(
        plan.version == 2 && plan.actions.len() <= 256,
        "unsupported or oversized replay plan",
    )?;
    let mut driver = Driver::new(path)?;
    let mut trace = Trace {
        reopen_observations: vec![],
        format: "brook-host-simulation-v2".into(),
        plan: plan.clone(),
        graphs: (0..2)
            .map(|n| {
                driver
                    .store
                    .effective_graph(&format!("namespace-{n}"), "terminal", 1)
            })
            .collect::<brook::Result<_>>()?,
        limits: Limits::default(),
        processing_limits: ProcessingLimits::default(),
        works: vec![],
        frames: vec![],
        failure: None,
    };
    let mut last_good = driver.snapshot()?;
    for (index, action) in plan.actions.iter().enumerate() {
        let result = driver.action(action);
        trace.works = driver.works.clone();
        trace.reopen_observations = driver.reopen_observations.clone();
        let assertions = match result {
            Ok(assertions) => assertions,
            Err(e) => {
                trace.failure = Some(format!("action {index} {action:?}: {e}"));
                vec![format!("action returned error: {e}")]
            }
        };
        let (observed, observation_failure) = match observe(&driver) {
            Ok(snapshot) => {
                last_good = snapshot.clone();
                (snapshot, None)
            }
            Err(e) => {
                let message = format!("snapshot after action {index} failed: {e}");
                if trace.failure.is_none() {
                    trace.failure = Some(message.clone());
                }
                (last_good.clone(), Some(message))
            }
        };
        trace.frames.push(Frame {
            action: action.clone(),
            observed,
            observation_failure,
            assertions,
        });
        if trace.failure.is_none() {
            if let Err(e) = verify(&trace, false) {
                trace.failure = Some(format!("action {index}: oracle: {e}"));
            }
        }
        if let Some(path) = trace_path {
            std::fs::write(path, serde_json::to_vec_pretty(&trace)?)?;
        }
        if trace.failure.is_some() {
            return Ok(trace);
        }
    }
    if let Err(e) = verify(&trace, true) {
        trace.failure = Some(e.to_string());
    }
    if let Some(path) = trace_path {
        std::fs::write(path, serde_json::to_vec_pretty(&trace)?)?;
    }
    Ok(trace)
}
