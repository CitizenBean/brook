use super::types::*;
use crate::{Boundary, Error, Result, SendOutcome, Store};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use serde_json::Value;
use std::{collections::BTreeSet, io::Write};

/// Render event text as data, never as terminal instructions. Keep ordinary text,
/// newlines and tabs; make controls and bidi direction overrides visible.
fn terminal_text(payload: &str) -> String {
    let mut rendered = String::with_capacity(payload.len());
    for ch in payload.chars() {
        let unsafe_control = ch.is_control() && ch != '\n' && ch != '\t';
        let direction_control = matches!(ch, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}');
        if unsafe_control || direction_control {
            rendered.extend(ch.escape_unicode());
        } else {
            rendered.push(ch);
        }
    }
    rendered
}

fn encode<T: Serialize>(v: &T, budget: usize) -> Result<String> {
    let text = serde_json::to_string(v)?;
    if text.len() > budget {
        return Err(Error::Capacity);
    }
    Ok(text)
}
fn limits(db: &Connection) -> Result<ProcessingLimits> {
    let text: String = db.query_row(
        "SELECT limits FROM processing_meta WHERE id=1 AND version=1",
        [],
        |r| r.get(0),
    )?;
    Ok(serde_json::from_str(&text)?)
}
fn store_id(db: &Connection) -> Result<String> {
    Ok(db.query_row("SELECT store_id FROM meta WHERE id=1", [], |r| r.get(0))?)
}
fn graph(db: &Connection, namespace: &str, pipeline: &str, version: u32) -> Result<Graph> {
    let body: String = db.query_row(
        "SELECT body FROM processing_graphs WHERE namespace=? AND pipeline=? AND version=?",
        params![namespace, pipeline, version],
        |r| r.get(0),
    )?;
    Ok(serde_json::from_str(&body)?)
}
fn capacity(db: &Connection, amount: usize, l: &ProcessingLimits) -> Result<()> {
    let count: usize = db.query_row("SELECT count(*) FROM processing_deliveries", [], |r| {
        r.get(0)
    })?;
    if count.saturating_add(amount) > l.deliveries {
        return Err(Error::Capacity);
    }
    Ok(())
}
fn authorized(db: &Connection, binding: &Version, origin: &TerminalIdentity) -> Result<()> {
    let valid: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM terminal_bindings WHERE id=? AND version=? AND identity=? AND active=1)", params![binding.id,binding.version,serde_json::to_string(origin)?], |r| r.get(0))?;
    if !valid {
        return Err(Error::Unauthorized);
    }
    Ok(())
}
fn fence(db: &Connection, lease: &ProcessingLease, incarnation: i64, now: i64) -> Result<()> {
    let s = &lease.scope;
    let valid: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM processing_state WHERE namespace=? AND pipeline=? AND node=? AND key=? AND generation=? AND owner=? AND deadline>?)", params![s.namespace,s.pipeline,s.node,s.key,lease.generation,lease.worker,now], |r| r.get(0))?;
    if lease.incarnation != incarnation || lease.store != store_id(db)? || !valid {
        return Err(Error::Fenced);
    }
    Ok(())
}
fn row(db: &Connection, id: DeliveryId) -> Result<(Scope, u32, TerminalIdentity, ProcessingEvent)> {
    let (ns,pipeline,node,key,v,origin,event): (String,String,String,String,u32,String,String) = db.query_row("SELECT namespace,pipeline,node,key,graph_version,origin,event FROM processing_deliveries WHERE id=?", [id.0], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?)))?;
    Ok((
        Scope {
            namespace: ns,
            pipeline,
            node,
            key,
        },
        v,
        serde_json::from_str(&origin)?,
        serde_json::from_str(&event)?,
    ))
}
fn insert(
    db: &Connection,
    g: &Graph,
    node: &str,
    origin: &TerminalIdentity,
    event: &ProcessingEvent,
    parent: Option<DeliveryId>,
    l: &ProcessingLimits,
) -> Result<DeliveryId> {
    let n = &g.nodes[node];
    if event.envelope_version != 1
        || event.schema != n.input.identity
        || !n.input.accepts(&event.payload)
    {
        return Err(Error::Invalid("event schema mismatch"));
    }
    if let NodeKind::Terminal { binding } = &n.kind {
        authorized(db, binding, origin)?;
    }
    let event = encode(event, l.input_bytes)?;
    db.execute("INSERT INTO processing_deliveries(namespace,pipeline,graph_version,node,key,origin,event,parent,reserved_bytes) VALUES(?,?,?,?,?,?,?,?,?)",params![g.namespace,g.pipeline,g.version,node,origin.key(),serde_json::to_string(origin)?,event,parent.map(|p|p.0),l.reservation_bytes()])?;
    let id = DeliveryId(db.last_insert_rowid());
    db.execute(
        "INSERT OR IGNORE INTO processing_state(namespace,pipeline,node,key,value,schema) VALUES(?,?,?,?,?,?)",
        params![g.namespace, g.pipeline, node, origin.key(), serde_json::to_string(&n.initial_state)?,serde_json::to_string(&n.state_schema)?],
    )?;
    let saved: String = db.query_row(
        "SELECT schema FROM processing_state WHERE namespace=? AND pipeline=? AND node=? AND key=?",
        params![g.namespace, g.pipeline, node, origin.key()],
        |r| r.get(0),
    )?;
    if serde_json::from_str::<Schema>(&saved)? != n.state_schema {
        return Err(Error::Invalid(
            "state schema change requires migration or new node",
        ));
    }
    Ok(id)
}
fn outcome(db: &Connection, id: DeliveryId) -> Result<Option<CommittedOutcome>> {
    let text: Option<String> = db.query_row(
        "SELECT outcome FROM processing_deliveries WHERE id=?",
        [id.0],
        |r| r.get(0),
    )?;
    text.map(|s| serde_json::from_str(&s).map_err(Error::from))
        .transpose()
}
fn current_attempt(db: &Connection, a: &ProcessingAttempt) -> Result<()> {
    let valid: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM processing_deliveries WHERE id=? AND status='running' AND attempts=? AND incarnation=? AND generation=?)",params![a.id.0,a.attempt,a.lease.incarnation,a.lease.generation],|r|r.get(0))?;
    if !valid {
        return Err(Error::NotReady);
    }
    Ok(())
}
fn targets(a: &ProcessingAttempt, p: &Proposal) -> Result<Vec<String>> {
    let n = &a.node;
    let targets = match &p.routing {
        Routing::Suppress(reason) => {
            if reason.is_empty() || reason.len() > 256 || !p.outputs.is_empty() {
                return Err(Error::Invalid(
                    "suppression requires reason and zero outputs",
                ));
            }
            vec![]
        }
        Routing::ConfiguredNext => {
            if p.outputs.is_empty() || n.next.is_empty() {
                return Err(Error::Invalid("use explicit suppression for zero outputs"));
            }
            n.next.clone()
        }
        Routing::SelectedBranches(branches) => {
            if !matches!(n.kind, NodeKind::Processor { routing: true }) {
                return Err(Error::Unauthorized);
            }
            if branches.is_empty()
                || branches.len() > a.limits.fanout
                || p.outputs.is_empty()
                || branches.iter().collect::<BTreeSet<_>>().len() != branches.len()
            {
                return Err(Error::Invalid("invalid branch selection"));
            }
            let mut selected = BTreeSet::new();
            for branch in branches {
                selected.extend(
                    n.branches
                        .get(branch)
                        .ok_or(Error::Invalid("unknown logical branch"))?
                        .iter()
                        .cloned(),
                );
            }
            selected.into_iter().collect()
        }
    };
    if targets.len().saturating_mul(p.outputs.len()) > a.limits.fanout {
        return Err(Error::Capacity);
    }
    Ok(targets)
}

impl Store {
    /// Configure processing quotas once. Existing values are immutable in this slice.
    pub fn configure_processing(&mut self, settings: ProcessingLimits) -> Result<()> {
        settings.validate()?;
        let (tx, _) = self.begin()?;
        let saved: Option<String> = tx
            .query_row("SELECT limits FROM processing_meta WHERE id=1", [], |r| {
                r.get(0)
            })
            .optional()?;
        if let Some(saved) = saved {
            if serde_json::from_str::<ProcessingLimits>(&saved)? != settings {
                return Err(Error::Conflict);
            }
        } else {
            tx.execute(
                "INSERT INTO processing_meta VALUES(1,1,?)",
                [serde_json::to_string(&settings)?],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
    /// Trusted control-plane verification, not authentication of payload claims.
    pub fn terminal_client(&self, verified: TerminalIdentity) -> Result<TerminalClient> {
        verified.validate()?;
        Ok(TerminalClient {
            store: store_id(&self.conn)?,
            identity: verified,
        })
    }
    pub fn bind_terminal(&mut self, binding: &Version, verified: &TerminalClient) -> Result<()> {
        binding.validate()?;
        if verified.store != store_id(&self.conn)? {
            return Err(Error::Unauthorized);
        }
        let l = limits(&self.conn)?;
        let (tx, _) = self.begin()?;
        let identity = serde_json::to_string(&verified.identity)?;
        let old: Option<String> = tx
            .query_row(
                "SELECT identity FROM terminal_bindings WHERE id=? AND version=?",
                params![binding.id, binding.version],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(old) = old {
            if old != identity {
                return Err(Error::Conflict);
            }
        } else {
            let count: usize =
                tx.query_row("SELECT count(*) FROM terminal_bindings", [], |r| r.get(0))?;
            if count >= l.bindings {
                return Err(Error::Capacity);
            }
            tx.execute(
                "INSERT INTO terminal_bindings VALUES(?,?,?,1)",
                params![binding.id, binding.version, identity],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn revoke_terminal(&mut self, binding: &Version) -> Result<()> {
        let (tx, _) = self.begin()?;
        tx.execute(
            "UPDATE terminal_bindings SET active=0 WHERE id=? AND version=?",
            params![binding.id, binding.version],
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn validate_graph(&self, graph: &Graph) -> Result<()> {
        graph.validate(&limits(&self.conn)?)
    }
    pub fn register_graph(&mut self, g: &Graph) -> Result<()> {
        let l = limits(&self.conn)?;
        g.validate(&l)?;
        let body = serde_json::to_string(g)?;
        let (tx, _) = self.begin()?;
        let old: Option<String> = tx
            .query_row(
                "SELECT body FROM processing_graphs WHERE namespace=? AND pipeline=? AND version=?",
                params![g.namespace, g.pipeline, g.version],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(old) = old {
            if serde_json::from_str::<Graph>(&old)? != *g {
                return Err(Error::Conflict);
            }
        } else {
            let count: usize =
                tx.query_row("SELECT count(*) FROM processing_graphs", [], |r| r.get(0))?;
            if count >= l.graphs {
                return Err(Error::Capacity);
            }
            for n in g.nodes.values() {
                if let NodeKind::Terminal { binding } = &n.kind {
                    let identity: String = tx.query_row(
                        "SELECT identity FROM terminal_bindings WHERE id=? AND version=?",
                        params![binding.id, binding.version],
                        |r| r.get(0),
                    )?;
                    if serde_json::from_str::<TerminalIdentity>(&identity)?.namespace != g.namespace
                    {
                        return Err(Error::Unauthorized);
                    }
                }
            }
            tx.execute(
                "INSERT INTO processing_graphs VALUES(?,?,?,?)",
                params![g.namespace, g.pipeline, g.version, body],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn effective_graph(&self, namespace: &str, pipeline: &str, version: u32) -> Result<Graph> {
        graph(&self.conn, namespace, pipeline, version)
    }
    pub fn submit_terminal(
        &mut self,
        client: &TerminalClient,
        pipeline: &str,
        version: u32,
        operation: &str,
        payload: Value,
    ) -> Result<DeliveryId> {
        name(operation)?;
        name(pipeline)?;
        if client.store != store_id(&self.conn)? {
            return Err(Error::Unauthorized);
        }
        let l = limits(&self.conn)?;
        // Fixed v1 canonical intent excludes future envelope defaults and generated metadata.
        let canonical = encode(&(1, pipeline, version, &payload), l.input_bytes)?;
        let hook = self.hook.clone();
        let (tx, _) = self.begin()?;
        let o = &client.identity;
        let old:Option<(i64,String)>=tx.query_row("SELECT delivery,canonical FROM processing_ingress WHERE namespace=? AND client=? AND conversation=? AND operation=?",params![o.namespace,o.client,o.conversation,operation],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
        if let Some((id, saved)) = old {
            if saved != canonical {
                return Err(Error::Conflict);
            }
            tx.commit()?;
            return Ok(DeliveryId(id));
        }
        let g = graph(&tx, &o.namespace, pipeline, version)?;
        capacity(&tx, 1, &l)?;
        let event = ProcessingEvent {
            envelope_version: 1,
            schema: g.nodes[&g.entry].input.identity.clone(),
            payload,
        };
        let id = insert(&tx, &g, &g.entry, o, &event, None, &l)?;
        tx.execute(
            "INSERT INTO processing_ingress VALUES(?,?,?,?,?,?)",
            params![
                o.namespace,
                o.client,
                o.conversation,
                operation,
                canonical,
                id.0
            ],
        )?;
        hook(Boundary::ProcessorIngressBeforeCommit);
        tx.commit()?;
        hook(Boundary::ProcessorIngressAfterCommit);
        Ok(id)
    }
    pub fn claim_processing(
        &mut self,
        id: DeliveryId,
        worker: &str,
        duration_ms: i64,
    ) -> Result<ProcessingLease> {
        name(worker)?;
        if !(1..=3600000).contains(&duration_ms) {
            return Err(Error::Invalid("lease duration"));
        }
        let incarnation = self.incarnation;
        let (tx, now) = self.begin()?;
        let (s, _, _, _) = row(&tx, id)?;
        let (generation,owner,deadline):(i64,Option<String>,i64)=tx.query_row("SELECT generation,owner,deadline FROM processing_state WHERE namespace=? AND pipeline=? AND node=? AND key=?",params![s.namespace,s.pipeline,s.node,s.key],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
        if owner.is_some() && deadline > now {
            return Err(Error::NotReady);
        }
        let generation = generation.checked_add(1).ok_or(Error::Capacity)?;
        let deadline = now.checked_add(duration_ms).ok_or(Error::Capacity)?;
        tx.execute("UPDATE processing_state SET generation=?,owner=?,deadline=? WHERE namespace=? AND pipeline=? AND node=? AND key=?",params![generation,worker,deadline,s.namespace,s.pipeline,s.node,s.key])?;
        tx.execute("UPDATE processing_deliveries SET status=CASE WHEN status IN ('printing','write_started') THEN 'unknown' ELSE 'pending' END, failure=CASE WHEN status IN ('printing','write_started') THEN 'lease expired during terminal attempt' ELSE failure END WHERE namespace=? AND pipeline=? AND node=? AND key=? AND status IN ('running','printing','write_started')",params![s.namespace,s.pipeline,s.node,s.key])?;
        let lease = ProcessingLease {
            store: store_id(&tx)?,
            incarnation,
            generation,
            worker: worker.into(),
            scope: s,
        };
        tx.commit()?;
        Ok(lease)
    }
    pub fn heartbeat_processing(
        &mut self,
        lease: &ProcessingLease,
        duration_ms: i64,
    ) -> Result<()> {
        if !(1..=3600000).contains(&duration_ms) {
            return Err(Error::Invalid("lease duration"));
        }
        let incarnation = self.incarnation;
        let (tx, now) = self.begin()?;
        fence(&tx, lease, incarnation, now)?;
        let s = &lease.scope;
        let deadline = now.checked_add(duration_ms).ok_or(Error::Capacity)?;
        tx.execute("UPDATE processing_state SET deadline=? WHERE namespace=? AND pipeline=? AND node=? AND key=?",params![deadline,s.namespace,s.pipeline,s.node,s.key])?;
        tx.commit()?;
        Ok(())
    }
    pub fn release_processing(&mut self, lease: &ProcessingLease) -> Result<()> {
        let incarnation = self.incarnation;
        let (tx, now) = self.begin()?;
        fence(&tx, lease, incarnation, now)?;
        let s = &lease.scope;
        // Released attempts are recoverable only after a fresh generation is claimed.
        tx.execute("UPDATE processing_state SET owner=NULL,deadline=0 WHERE namespace=? AND pipeline=? AND node=? AND key=?",params![s.namespace,s.pipeline,s.node,s.key])?;
        tx.commit()?;
        Ok(())
    }
    pub fn prepare_processing(
        &mut self,
        lease: &ProcessingLease,
        id: DeliveryId,
    ) -> Result<ProcessingAttempt> {
        let l = limits(&self.conn)?;
        let incarnation = self.incarnation;
        let (tx, now) = self.begin()?;
        fence(&tx, lease, incarnation, now)?;
        let (s, v, origin, event) = row(&tx, id)?;
        if s != lease.scope {
            return Err(Error::Unauthorized);
        }
        let g = graph(&tx, &s.namespace, &s.pipeline, v)?;
        let node = g.nodes[&s.node].clone();
        if !matches!(node.kind, NodeKind::Processor { .. }) {
            return Err(Error::Invalid("terminal is an effect adapter"));
        }
        let (status, count): (String, u32) = tx.query_row(
            "SELECT status,attempts FROM processing_deliveries WHERE id=?",
            [id.0],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if status != "pending" {
            return Err(Error::NotReady);
        }
        if count >= l.attempts {
            tx.execute("UPDATE processing_deliveries SET status='failed',failure='attempt budget exhausted' WHERE id=?",[id.0])?;
            tx.commit()?;
            return Err(Error::Capacity);
        }
        let(revision,state):(i64,String)=tx.query_row("SELECT revision,value FROM processing_state WHERE namespace=? AND pipeline=? AND node=? AND key=?",params![s.namespace,s.pipeline,s.node,s.key],|r|Ok((r.get(0)?,r.get(1)?)))?;
        tx.execute("UPDATE processing_deliveries SET status='running',attempts=attempts+1,incarnation=?,generation=? WHERE id=?",params![incarnation,lease.generation,id.0])?;
        let a = ProcessingAttempt {
            id,
            lease: lease.clone(),
            attempt: count + 1,
            revision,
            event,
            state: serde_json::from_str(&state)?,
            node,
            graph: g,
            origin,
            limits: l,
        };
        tx.commit()?;
        Ok(a)
    }
    pub fn execute_processor(
        &self,
        a: &ProcessingAttempt,
        processor: &dyn Processor,
    ) -> Result<Proposal> {
        if processor.code() != a.node.code {
            return Err(Error::Invalid(
                "processor code version differs from pinned node",
            ));
        }
        processor.process(a)
    }
    pub fn commit_processing(
        &mut self,
        a: &ProcessingAttempt,
        p: &Proposal,
    ) -> Result<CommittedOutcome> {
        // Committed duplicates recover the authoritative outcome, never reapply a mutation.
        if a.lease.store != store_id(&self.conn)? {
            return Err(Error::Unauthorized);
        }
        if let Some(done) = outcome(&self.conn, a.id)? {
            return Ok(done);
        }
        let destinations = targets(a, p)?;
        let state = encode(&p.state, a.limits.state_bytes)?;
        if !a.node.state_schema.accepts(&p.state) {
            return Err(Error::Invalid("state schema mismatch"));
        }
        if p.reason.is_empty() || p.reason.len() > 256 {
            return Err(Error::Invalid("routing reason required"));
        }
        encode(p, a.limits.output_bytes)?;
        for output in &p.outputs {
            if !a.node.output.accepts(output) {
                return Err(Error::Invalid("output schema mismatch"));
            }
        }
        let hook = self.hook.clone();
        let incarnation = self.incarnation;
        let (tx, now) = self.begin()?;
        fence(&tx, &a.lease, incarnation, now)?;
        current_attempt(&tx, a)?;
        let s = &a.lease.scope;
        let revision:i64=tx.query_row("SELECT revision FROM processing_state WHERE namespace=? AND pipeline=? AND node=? AND key=?",params![s.namespace,s.pipeline,s.node,s.key],|r|r.get(0))?;
        if revision != a.revision {
            tx.execute(
                "UPDATE processing_deliveries SET status=?,failure='state conflict' WHERE id=?",
                params![
                    if a.attempt >= a.limits.attempts {
                        "failed"
                    } else {
                        "pending"
                    },
                    a.id.0
                ],
            )?;
            tx.commit()?;
            return Err(Error::RevisionChanged);
        }
        capacity(&tx, destinations.len() * p.outputs.len(), &a.limits)?;
        let mut deliveries = Vec::new();
        for output in &p.outputs {
            for dest in &destinations {
                let event = ProcessingEvent {
                    envelope_version: 1,
                    schema: a.node.output.identity.clone(),
                    payload: output.clone(),
                };
                deliveries.push(insert(
                    &tx,
                    &a.graph,
                    dest,
                    &a.origin,
                    &event,
                    Some(a.id),
                    &a.limits,
                )?);
                if deliveries.len() == 1 {
                    hook(Boundary::ProcessorAfterFirstChild);
                }
            }
        }
        let done = CommittedOutcome {
            decision: p.clone(),
            deliveries,
        };
        let serialized = encode(&done, a.limits.output_bytes + a.limits.fanout * 64 + 512)?;
        tx.execute("UPDATE processing_state SET value=?,revision=? WHERE namespace=? AND pipeline=? AND node=? AND key=?",params![state,revision.checked_add(1).ok_or(Error::Capacity)?,s.namespace,s.pipeline,s.node,s.key])?;
        tx.execute(
            "UPDATE processing_deliveries SET status='done',outcome=?,failure=NULL WHERE id=?",
            params![serialized, a.id.0],
        )?;
        hook(Boundary::ProcessorBeforeCommit);
        tx.commit()?;
        hook(Boundary::ProcessorAfterCommit);
        Ok(done)
    }
    pub fn fail_processing(
        &mut self,
        a: &ProcessingAttempt,
        reason: &str,
        retry: bool,
    ) -> Result<()> {
        if reason.is_empty() || reason.len() > 256 {
            return Err(Error::Invalid("failure reason size"));
        }
        let incarnation = self.incarnation;
        let (tx, now) = self.begin()?;
        fence(&tx, &a.lease, incarnation, now)?;
        current_attempt(&tx, a)?;
        tx.execute(
            "UPDATE processing_deliveries SET status=?,failure=? WHERE id=?",
            params![
                if retry && a.attempt < a.limits.attempts {
                    "pending"
                } else {
                    "failed"
                },
                reason,
                a.id.0
            ],
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn begin_terminal(
        &mut self,
        lease: &ProcessingLease,
        id: DeliveryId,
        client: &TerminalClient,
    ) -> Result<TerminalAttempt> {
        let incarnation = self.incarnation;
        let hook = self.hook.clone();
        let (tx, now) = self.begin()?;
        fence(&tx, lease, incarnation, now)?;
        let (s, v, origin, event) = row(&tx, id)?;
        if s != lease.scope || client.store != lease.store || client.identity != origin {
            return Err(Error::Unauthorized);
        }
        let g = graph(&tx, &s.namespace, &s.pipeline, v)?;
        let NodeKind::Terminal { binding } = &g.nodes[&s.node].kind else {
            return Err(Error::Invalid("not terminal output"));
        };
        authorized(&tx, binding, &origin)?;
        let updated=tx.execute("UPDATE processing_deliveries SET status='printing',attempts=attempts+1,generation=?,incarnation=? WHERE id=? AND status='pending' AND attempts=0",params![lease.generation,incarnation,id.0])?;
        if updated != 1 {
            return Err(Error::NotReady);
        }
        let payload = event
            .payload
            .as_str()
            .ok_or(Error::Invalid("terminal text schema"))?
            .to_owned();
        hook(Boundary::TerminalBeforeCommit);
        tx.commit()?;
        hook(Boundary::TerminalAfterCommit);
        Ok(TerminalAttempt {
            id,
            lease: lease.clone(),
            payload,
            origin,
        })
    }
    pub fn finish_terminal(&mut self, a: &TerminalAttempt, result: SendOutcome) -> Result<()> {
        let incarnation = self.incarnation;
        let hook = self.hook.clone();
        let (tx, now) = self.begin()?;
        fence(&tx, &a.lease, incarnation, now)?;
        let (status, failure) = match result {
            SendOutcome::Applied => ("printed", None),
            SendOutcome::ConfirmedNotApplied => ("failed", Some("confirmed no output")),
            SendOutcome::Unknown => ("unknown", Some("terminal output uncertain")),
        };
        let n=tx.execute("UPDATE processing_deliveries SET status=?,failure=? WHERE id=? AND status='write_started' AND generation=? AND incarnation=?",params![status,failure,a.id.0,a.lease.generation,incarnation])?;
        if n != 1 {
            return Err(Error::NotReady);
        }
        hook(Boundary::TerminalFinishBeforeCommit);
        tx.commit()?;
        hook(Boundary::TerminalFinishAfterCommit);
        Ok(())
    }
    /// Any write/flush failure may follow partial output. Never infer non-application.
    pub fn print_terminal(&mut self, a: &TerminalAttempt, writer: &mut impl Write) -> Result<()> {
        // Consume permission durably before I/O. A failed completion transaction must
        // never make this clonable attempt eligible to write a second time.
        let incarnation = self.incarnation;
        let hook = self.hook.clone();
        let (tx, now) = self.begin()?;
        fence(&tx, &a.lease, incarnation, now)?;
        let updated = tx.execute("UPDATE processing_deliveries SET status='write_started' WHERE id=? AND status='printing' AND generation=? AND incarnation=?",params![a.id.0,a.lease.generation,incarnation])?;
        if updated != 1 {
            return Err(Error::NotReady);
        }
        hook(Boundary::TerminalWriteBeforeCommit);
        tx.commit()?;
        hook(Boundary::TerminalWriteAfterCommit);
        let result = writeln!(writer, "{}", terminal_text(&a.payload)).and_then(|_| writer.flush());
        (self.hook)(Boundary::TerminalWritten);
        self.finish_terminal(
            a,
            if result.is_ok() {
                SendOutcome::Applied
            } else {
                SendOutcome::Unknown
            },
        )?;
        result.map_err(Error::Io)
    }
    pub fn pending_processing(&self, limit: usize) -> Result<Vec<DeliveryId>> {
        if !(1..=64).contains(&limit) {
            return Err(Error::Invalid("scan limit"));
        }
        let mut q = self.conn.prepare(
            "SELECT id FROM processing_deliveries WHERE status='pending' ORDER BY id LIMIT ?",
        )?;
        let rows = q.query_map([limit], |r| Ok(DeliveryId(r.get(0)?)))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }
    /// Scope the bundled terminal runner to its verified connection and pipeline.
    pub fn pending_terminal(
        &self,
        client: &TerminalClient,
        pipeline: &str,
        limit: usize,
    ) -> Result<Vec<DeliveryId>> {
        if client.store != store_id(&self.conn)? {
            return Err(Error::Unauthorized);
        }
        name(pipeline)?;
        if !(1..=64).contains(&limit) {
            return Err(Error::Invalid("scan limit"));
        }
        let mut q = self.conn.prepare("SELECT id FROM processing_deliveries WHERE status='pending' AND origin=? AND pipeline=? ORDER BY id LIMIT ?")?;
        let rows = q.query_map(
            params![serde_json::to_string(&client.identity)?, pipeline, limit],
            |r| Ok(DeliveryId(r.get(0)?)),
        )?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }
    /// Stable identities and status only; event payload and state are excluded by default.
    pub fn inspect_processing(&self, id: DeliveryId) -> Result<DeliveryStatus> {
        let (scope, graph_version, _, _) = row(&self.conn, id)?;
        let (status, attempts, failure, parent): (String, u32, Option<String>, Option<i64>) =
            self.conn.query_row(
                "SELECT status,attempts,failure,parent FROM processing_deliveries WHERE id=?",
                [id.0],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )?;
        let done = outcome(&self.conn, id)?;
        Ok(DeliveryStatus {
            id,
            scope,
            graph_version,
            status,
            attempts,
            failure,
            parent: parent.map(DeliveryId),
            reason: done.as_ref().map(|d| d.decision.reason.clone()),
            children: done.map(|d| d.deliveries).unwrap_or_default(),
        })
    }
    pub fn committed_processing(&self, id: DeliveryId) -> Result<Option<CommittedOutcome>> {
        outcome(&self.conn, id)
    }
}
