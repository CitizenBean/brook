use crate::*;
use fs2::FileExt;
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use std::{
    fs::{File, OpenOptions},
    path::Path,
    sync::Arc,
};

type Hook = Arc<dyn Fn(Boundary) + Send + Sync>;
/// One authority per canonical directory. Share via a mutex, never open a worker DB.
/// Local trusted host APIs; ingress authentication and policy evaluation are external.
pub struct Store {
    pub(crate) conn: Connection,
    _lock: File,
    limits: Limits,
    clock: Arc<dyn Clock>,
    clock_base: i64,
    pub(crate) incarnation: i64,
    pub(crate) hook: Hook,
}

fn bounded(text: &str, max: usize) -> Result<()> {
    if text.is_empty() || text.len() > max {
        Err(Error::Invalid("empty or oversized field"))
    } else {
        Ok(())
    }
}
fn increment(value: i64) -> Result<i64> {
    value.checked_add(1).ok_or(Error::Capacity)
}
fn deadline(now: i64, duration: i64) -> Result<i64> {
    if !(1..=31_536_000_000).contains(&duration) {
        return Err(Error::Invalid("duration out of range"));
    }
    now.checked_add(duration).ok_or(Error::Capacity)
}
fn session_state(tx: &Connection, session: SessionId) -> Result<(i64, i64)> {
    Ok(tx.query_row(
        "SELECT revision,cancellation FROM sessions WHERE id=?",
        [session.0],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?)
}
fn fence(tx: &Connection, lease: &Lease, incarnation: i64, now: i64) -> Result<()> {
    let valid: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM sessions WHERE id=?1 AND owner=?2 AND generation=?3 AND deadline>?4)",
        params![lease.session.0, lease.worker, lease.generation, now], |r| r.get(0))?;
    let store_id: String =
        tx.query_row("SELECT store_id FROM meta WHERE id=1", [], |r| r.get(0))?;
    if lease.store_id != store_id || lease.incarnation != incarnation || !valid {
        Err(Error::Fenced)
    } else {
        Ok(())
    }
}
fn append(
    tx: &Connection,
    session: SessionId,
    kind: &str,
    text: &str,
    request: Option<i64>,
) -> Result<i64> {
    let (revision, _) = session_state(tx, session)?;
    let revision = increment(revision)?;
    tx.execute(
        "UPDATE sessions SET revision=? WHERE id=?",
        params![revision, session.0],
    )?;
    tx.execute(
        "INSERT INTO events(session,revision,kind,text,request) VALUES(?,?,?,?,?)",
        params![session.0, revision, kind, text, request],
    )?;
    Ok(tx.last_insert_rowid())
}
fn submission(tx: &Connection, request: i64) -> Result<Submission> {
    let text: String = tx.query_row(
        "SELECT canonical FROM requests WHERE id=?",
        [request],
        |r| r.get(0),
    )?;
    Ok(serde_json::from_str(&text)?)
}
fn history(tx: &Connection, session: SessionId) -> Result<Vec<Event>> {
    let mut query = tx.prepare(
        "SELECT id,revision,kind,text,request FROM events WHERE session=? ORDER BY revision",
    )?;
    let rows = query.query_map([session.0], |r| {
        Ok(Event {
            id: r.get(0)?,
            revision: r.get(1)?,
            kind: r.get(2)?,
            text: r.get(3)?,
            request: r.get(4)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}
fn authorize(tx: &Connection, session: SessionId, input: &Submission, now: i64) -> Result<()> {
    let target = serde_json::to_string(&input.destination)?;
    let valid: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM grants WHERE id=?1 AND session=?2 AND version=?3 AND destination=?4 AND content=?5 AND expires>?6 AND active=1)",
        params![input.grant,session.0,input.grant_version,target,input.payload,now], |r| r.get(0))?;
    if valid {
        Ok(())
    } else {
        Err(Error::Unauthorized)
    }
}
fn request_open(
    tx: &Connection,
    request: i64,
    session: SessionId,
    now: i64,
    state: &str,
) -> Result<()> {
    let valid: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM requests r JOIN sessions s ON s.id=r.session WHERE r.id=?1 AND r.session=?2 AND r.state=?3 AND r.deadline>?4 AND r.cancellation=s.cancellation)",
        params![request,session.0,state,now], |r| r.get(0))?;
    if valid {
        Ok(())
    } else {
        Err(Error::NotReady)
    }
}
fn quarantine(tx: &Connection, request: i64, classification: &str) -> Result<()> {
    tx.execute(
        "INSERT OR IGNORE INTO recovery(request,classification) VALUES(?,?)",
        params![request, classification],
    )?;
    Ok(())
}
fn release_pins(tx: &Connection, request: i64) -> Result<()> {
    // Every dependency must be settled. A reply/job can finish while its send is
    // still in flight; that delivery may subsequently need recovery evidence.
    tx.execute(
        "DELETE FROM pins WHERE request=?1
         AND EXISTS(SELECT 1 FROM requests WHERE id=?1 AND state IN ('done','cancelled','expired','recovery_failed'))
         AND NOT EXISTS(SELECT 1 FROM jobs WHERE request=?1 AND state IN ('pending','ready','running'))
         AND EXISTS(SELECT 1 FROM outbox WHERE request=?1 AND state IN ('sent','cancelled'))
         AND NOT EXISTS(SELECT 1 FROM recovery WHERE request=?1)",
        [request],
    )?;
    Ok(())
}

impl Store {
    pub fn open(path: impl AsRef<Path>, limits: Limits, clock: Arc<dyn Clock>) -> Result<Self> {
        limits.validate()?;
        std::fs::create_dir_all(path.as_ref())?;
        let path = path.as_ref().canonicalize()?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path.join("authority.lock"))?;
        lock.try_lock_exclusive().map_err(|e| {
            if e.kind() == std::io::ErrorKind::WouldBlock {
                Error::Locked
            } else {
                Error::Io(e)
            }
        })?;
        let mut conn = Connection::open(path.join("brook.sqlite3"))?;
        let existing_meta: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='meta')",
            [],
            |r| r.get(0),
        )?;
        if existing_meta {
            let version: i64 =
                conn.query_row("SELECT schema_version FROM meta WHERE id=1", [], |r| {
                    r.get(0)
                })?;
            if ![1, 2].contains(&version) {
                return Err(Error::Invalid("unsupported database schema"));
            }
            if version == 2 {
                let processing: bool = conn.query_row(
                    "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='processing_meta')",
                    [],
                    |r| r.get(0),
                )?;
                if !processing {
                    return Err(Error::Invalid("incompatible experimental schema v2"));
                }
                let unsupported: bool = conn.query_row(
                    "SELECT EXISTS(SELECT 1 FROM processing_meta WHERE version != 1)",
                    [],
                    |r| r.get(0),
                )?;
                if unsupported {
                    return Err(Error::Invalid("unsupported processing schema"));
                }
            }
        }
        conn.execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA wal_autocheckpoint=64; PRAGMA journal_size_limit=1048576; PRAGMA cache_size=-2048;")?;

        let config = serde_json::to_string(&limits)?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute_batch(include_str!("schema.sql"))?;
        tx.execute(
            "INSERT OR IGNORE INTO meta VALUES(1,1,lower(hex(randomblob(16))),0,0,?)",
            [&config],
        )?;
        let (schema, incarnation, clock_base, saved): (i64, i64, i64, String) = tx.query_row(
            "SELECT schema_version,incarnation,tick,limits FROM meta WHERE id=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )?;
        if ![1, 2].contains(&schema) || serde_json::from_str::<Limits>(&saved)? != limits {
            return Err(Error::Invalid("schema or persisted limits mismatch"));
        }
        // Transactional v1 -> v2 migration leaves every original canonical request unchanged.
        tx.execute_batch(include_str!("processing/schema.sql"))?;
        tx.execute("UPDATE meta SET schema_version=2 WHERE id=1", [])?;
        tx.execute("UPDATE processing_state SET owner=NULL,deadline=0", [])?;
        tx.execute(
            "UPDATE processing_deliveries SET status='pending' WHERE status='running'",
            [],
        )?;
        tx.execute("UPDATE processing_deliveries SET status='unknown',failure='interrupted terminal attempt' WHERE status IN ('printing','write_started')", [])?;
        let incarnation = increment(incarnation)?;
        tx.execute("UPDATE meta SET incarnation=? WHERE id=1", [incarnation])?;
        tx.execute("UPDATE sessions SET owner=NULL,deadline=0", [])?;
        tx.execute("INSERT OR IGNORE INTO recovery(request,classification) SELECT request,'unknown' FROM outbox WHERE state='in_flight'", [])?;
        tx.execute(
            "UPDATE outbox SET state='unknown' WHERE state='in_flight'",
            [],
        )?;
        tx.execute("UPDATE jobs SET state='ready' WHERE state='running'", [])?;
        tx.commit()?;
        Ok(Self {
            conn,
            _lock: lock,
            limits,
            clock,
            clock_base,
            incarnation,
            hook: Arc::new(|_| {}),
        })
    }
    pub fn set_boundary_hook(&mut self, hook: impl Fn(Boundary) + Send + Sync + 'static) {
        self.hook = Arc::new(hook);
    }
    pub fn limits(&self) -> &Limits {
        &self.limits
    }
    pub(crate) fn begin(&mut self) -> Result<(Transaction<'_>, i64)> {
        let elapsed = self.clock.elapsed_ms();
        if elapsed < 0 {
            return Err(Error::Invalid("clock regressed"));
        }
        let now = self
            .clock_base
            .checked_add(elapsed)
            .ok_or(Error::Capacity)?;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (previous, incarnation): (i64, i64) =
            tx.query_row("SELECT tick,incarnation FROM meta WHERE id=1", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?;
        if now < previous || incarnation != self.incarnation {
            return Err(Error::Fenced);
        }
        tx.execute("UPDATE meta SET tick=? WHERE id=1", [now])?;
        Ok((tx, now))
    }
    /// Inputs must come from a trusted adapter connection, never message payload claims.
    pub fn resolve_session(
        &mut self,
        namespace: &str,
        client: &str,
        external_id: &str,
    ) -> Result<SessionId> {
        for field in [namespace, client, external_id] {
            bounded(field, 256)?;
        }
        let cap = self.limits.sessions;
        let (tx, _) = self.begin()?;
        if let Some(id) = tx
            .query_row(
                "SELECT id FROM sessions WHERE namespace=? AND client=? AND external_id=?",
                params![namespace, client, external_id],
                |r| r.get(0),
            )
            .optional()?
        {
            tx.commit()?;
            return Ok(SessionId(id));
        }
        let count: i64 = tx.query_row("SELECT count(*) FROM sessions", [], |r| r.get(0))?;
        if count >= cap {
            return Err(Error::Capacity);
        }
        tx.execute(
            "INSERT INTO sessions(namespace,client,external_id) VALUES(?,?,?)",
            params![namespace, client, external_id],
        )?;
        let id = SessionId(tx.last_insert_rowid());
        tx.commit()?;
        Ok(id)
    }
    pub fn claim(&mut self, session: SessionId, worker: &str, duration: i64) -> Result<Lease> {
        bounded(worker, 256)?;
        let incarnation = self.incarnation;
        let (tx, now) = self.begin()?;
        let (generation, owner, expiry): (i64, Option<String>, i64) = tx.query_row(
            "SELECT generation,owner,deadline FROM sessions WHERE id=?",
            [session.0],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        if owner.is_some() && now < expiry {
            return Err(Error::Fenced);
        }
        let generation = increment(generation)?;
        tx.execute(
            "UPDATE sessions SET generation=?,owner=?,deadline=? WHERE id=?",
            params![generation, worker, deadline(now, duration)?, session.0],
        )?;
        tx.execute("INSERT OR IGNORE INTO recovery(request,classification) SELECT o.request,'unknown' FROM outbox o JOIN requests r ON r.id=o.request WHERE r.session=? AND o.state='in_flight'",[session.0])?;
        tx.execute("UPDATE outbox SET state='unknown' WHERE state='in_flight' AND request IN (SELECT id FROM requests WHERE session=?)",[session.0])?;
        tx.execute("UPDATE jobs SET state='ready' WHERE state='running' AND request IN (SELECT id FROM requests WHERE session=?)",[session.0])?;
        let store_id = tx.query_row("SELECT store_id FROM meta WHERE id=1", [], |r| r.get(0))?;
        tx.commit()?;
        Ok(Lease {
            session,
            worker: worker.to_owned(),
            generation,
            incarnation,
            store_id,
        })
    }
    pub fn heartbeat(&mut self, lease: &Lease, duration: i64) -> Result<()> {
        let incarnation = self.incarnation;
        let (tx, now) = self.begin()?;
        fence(&tx, lease, incarnation, now)?;
        tx.execute(
            "UPDATE sessions SET deadline=? WHERE id=?",
            params![deadline(now, duration)?, lease.session.0],
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn release(&mut self, lease: &Lease) -> Result<()> {
        let incarnation = self.incarnation;
        let (tx, now) = self.begin()?;
        fence(&tx, lease, incarnation, now)?;
        // Recoverable attempts remain keyed to their old grant and cannot commit.
        tx.execute(
            "UPDATE sessions SET owner=NULL,deadline=0 WHERE id=?",
            [lease.session.0],
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn message(&mut self, session: SessionId, text: &str) -> Result<i64> {
        bounded(text, self.limits.payload_bytes)?;
        let cap = self.limits.messages_per_session;
        let (tx, _) = self.begin()?;
        let count: i64 = tx.query_row(
            "SELECT count(*) FROM events WHERE session=? AND kind='message'",
            [session.0],
            |r| r.get(0),
        )?;
        if count >= cap {
            return Err(Error::Capacity);
        }
        let event = append(&tx, session, "message", text, None)?;
        tx.commit()?;
        Ok(event)
    }
    /// Configure/replace trusted local policy. Version increases even after revocation.
    pub fn grant(
        &mut self,
        id: &str,
        session: SessionId,
        target: &Destination,
        content: &str,
        duration: i64,
    ) -> Result<i64> {
        bounded(id, 256)?;
        for field in [&target.sink, &target.account, &target.recipient] {
            bounded(field, 256)?;
        }
        if target.sink != "fake" {
            return Err(Error::Invalid("only fake sink supported"));
        }
        bounded(content, self.limits.payload_bytes)?;
        let cap = self.limits.grants;
        let (tx, now) = self.begin()?;
        session_state(&tx, session)?;
        let old: Option<i64> = tx
            .query_row("SELECT version FROM grants WHERE id=?", [id], |r| r.get(0))
            .optional()?;
        if old.is_none() {
            let count: i64 = tx.query_row("SELECT count(*) FROM grants", [], |r| r.get(0))?;
            if count >= cap {
                return Err(Error::Capacity);
            }
        }
        let version = increment(old.unwrap_or(0))?;
        tx.execute("INSERT INTO grants VALUES(?,?,?,?,?,?,1) ON CONFLICT(id) DO UPDATE SET session=excluded.session,version=excluded.version,destination=excluded.destination,content=excluded.content,expires=excluded.expires,active=1",params![id,session.0,version,serde_json::to_string(target)?,content,deadline(now,duration)?])?;
        tx.commit()?;
        Ok(version)
    }
    pub fn revoke(&mut self, id: &str) -> Result<()> {
        let (tx, _) = self.begin()?;
        tx.execute("UPDATE grants SET active=0 WHERE id=?", [id])?;
        tx.commit()?;
        Ok(())
    }
    pub fn admit(&mut self, lease: &Lease, input: &Submission) -> Result<Receipt> {
        for field in [
            &input.agent.id,
            &input.agent.role,
            &input.agent.model,
            &input.operation,
            &input.grant,
            &input.expected_peer,
        ] {
            bounded(field, 256)?;
        }
        bounded(&input.agent.instructions, self.limits.payload_bytes)?;
        if input.agent.version < 1
            || input.agent.temperature_milli > 2000
            || !(1..=32768).contains(&input.agent.max_output_tokens)
            || !input.agent.allowed_tools.is_empty()
        {
            return Err(Error::Invalid(
                "this slice permits no harness tool execution",
            ));
        }
        bounded(&input.instruction, self.limits.payload_bytes)?;
        bounded(&input.payload, self.limits.payload_bytes)?;
        for field in [
            &input.destination.sink,
            &input.destination.account,
            &input.destination.recipient,
        ] {
            bounded(field, 256)?;
        }
        if input.causal_events.len() > 32 || input.causal_events.windows(2).any(|x| x[0] >= x[1]) {
            return Err(Error::Invalid(
                "causal IDs must be sorted and unique; at most 32",
            ));
        }
        // Versioned fixed-field struct serialization. No maps, floats or digest-only equality.
        let canonical = serde_json::to_string(input)?;
        let cap = self.limits.requests;
        let reservation = self.limits.request_reservation();
        let incarnation = self.incarnation;
        let hook = self.hook.clone();
        let (tx, now) = self.begin()?;
        fence(&tx, lease, incarnation, now)?;
        let old: Option<(i64, String)> = tx
            .query_row(
                "SELECT id,canonical FROM requests WHERE session=? AND operation=?",
                params![lease.session.0, input.operation],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((request, saved)) = old {
            if serde_json::from_str::<Submission>(&saved)? != *input {
                return Err(Error::Conflict);
            }
            tx.commit()?;
            return Ok(Receipt { request });
        }
        authorize(&tx, lease.session, input, now)?;
        let count: i64 = tx.query_row("SELECT count(*) FROM requests", [], |r| r.get(0))?;
        if count >= cap {
            return Err(Error::Capacity);
        }
        for event in &input.causal_events {
            let owned: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM events WHERE id=? AND session=?)",
                params![event, lease.session.0],
                |r| r.get(0),
            )?;
            if !owned {
                return Err(Error::Unauthorized);
            }
        }
        let (_, cancellation) = session_state(&tx, lease.session)?;
        tx.execute("INSERT INTO requests(session,operation,canonical,cancellation,deadline,state,reserved_bytes) VALUES(?,?,?,?,?,'pending',?)",params![lease.session.0,input.operation,canonical,cancellation,deadline(now,input.lifetime_ms)?,reservation])?;
        let request = tx.last_insert_rowid();
        let call = append(
            &tx,
            lease.session,
            "call",
            &input.instruction,
            Some(request),
        )?;
        for event in input
            .causal_events
            .iter()
            .copied()
            .chain(std::iter::once(call))
        {
            tx.execute("INSERT INTO pins VALUES(?,?)", params![request, event])?;
        }
        tx.execute(
            "INSERT INTO outbox(request,state) VALUES(?,'ready')",
            [request],
        )?;
        hook(Boundary::AdmissionBeforeCommit);
        tx.commit()?;
        hook(Boundary::AdmissionAfterCommit);
        Ok(Receipt { request })
    }
    pub fn begin_send(&mut self, lease: &Lease, receipt: Receipt) -> Result<SendAttempt> {
        let incarnation = self.incarnation;
        let hook = self.hook.clone();
        let (tx, now) = self.begin()?;
        fence(&tx, lease, incarnation, now)?;
        request_open(&tx, receipt.request, lease.session, now, "pending")?;
        let input = submission(&tx, receipt.request)?;
        authorize(&tx, lease.session, &input, now)?;
        let changed=tx.execute("UPDATE outbox SET state='in_flight',incarnation=?,generation=?,worker=? WHERE request=? AND state='ready'",params![incarnation,lease.generation,lease.worker,receipt.request])?;
        if changed != 1 {
            return Err(Error::NotReady);
        }
        hook(Boundary::SendBeforeCommit);
        tx.commit()?;
        hook(Boundary::SendAfterCommit);
        Ok(SendAttempt {
            request: receipt.request,
            lease: lease.clone(),
            destination: input.destination,
            payload: input.payload,
        })
    }
    pub fn finish_send(&mut self, attempt: &SendAttempt, outcome: SendOutcome) -> Result<()> {
        let hook = self.hook.clone();
        let incarnation = self.incarnation;
        let (tx, now) = self.begin()?;
        fence(&tx, &attempt.lease, incarnation, now)?;
        let state = match outcome {
            SendOutcome::Applied => "sent",
            SendOutcome::ConfirmedNotApplied => "failed",
            SendOutcome::Unknown => "unknown",
        };
        let changed=tx.execute("UPDATE outbox SET state=? WHERE request=? AND state='in_flight' AND incarnation=? AND generation=? AND worker=?",params![state,attempt.request,incarnation,attempt.lease.generation,attempt.lease.worker])?;
        if changed != 1 {
            return Err(Error::NotReady);
        }
        if state != "sent" {
            quarantine(&tx, attempt.request, state)?;
            hook(Boundary::QuarantineBeforeCommit);
        }
        release_pins(&tx, attempt.request)?;
        tx.commit()?;
        if state != "sent" {
            hook(Boundary::QuarantineAfterCommit);
        }
        Ok(())
    }
    /// Authenticated peer identity is a trusted ingress assertion, not a reply field.
    pub fn accept_reply(
        &mut self,
        receipt: Receipt,
        authenticated_peer: &str,
        outcome: &str,
    ) -> Result<bool> {
        bounded(outcome, self.limits.payload_bytes)?;
        let hook = self.hook.clone();
        let (tx, now) = self.begin()?;
        let (session, state, previous): (i64, String, Option<String>) = tx.query_row(
            "SELECT session,state,outcome FROM requests WHERE id=?",
            [receipt.request],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        let input = submission(&tx, receipt.request)?;
        if input.expected_peer != authenticated_peer {
            return Err(Error::Unauthorized);
        }
        if previous.as_deref() == Some(outcome) && (state == "accepted" || state == "done") {
            tx.commit()?;
            return Ok(false);
        }
        if previous.is_some() {
            return Err(Error::Conflict);
        }
        request_open(&tx, receipt.request, SessionId(session), now, "pending")?;
        let dispatched: bool = tx.query_row(
            "SELECT state IN ('sent','in_flight','unknown') FROM outbox WHERE request=?",
            [receipt.request],
            |r| r.get(0),
        )?;
        if !dispatched {
            return Err(Error::NotReady);
        }
        tx.execute(
            "UPDATE requests SET state='accepted',outcome=? WHERE id=?",
            params![outcome, receipt.request],
        )?;
        append(
            &tx,
            SessionId(session),
            "outcome",
            outcome,
            Some(receipt.request),
        )?;
        tx.execute(
            "INSERT INTO jobs(request,state) VALUES(?,'pending')",
            [receipt.request],
        )?;
        hook(Boundary::ReplyBeforeCommit);
        tx.commit()?;
        hook(Boundary::ReplyAfterCommit);
        Ok(true)
    }
    pub fn build_context(&self, receipt: Receipt) -> Result<Context> {
        let (session, state, outcome): (i64, String, Option<String>) = self.conn.query_row(
            "SELECT session,state,outcome FROM requests WHERE id=?",
            [receipt.request],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        if state != "accepted" {
            return Err(Error::NotReady);
        }
        let input = submission(&self.conn, receipt.request)?;
        let session = SessionId(session);
        let (revision, cancellation) = session_state(&self.conn, session)?;
        let mut query = self.conn.prepare(
            "SELECT id,revision,kind,text,request FROM events WHERE session=? ORDER BY revision",
        )?;
        let mut rows = query.query([session.0])?;
        let mut events = Vec::new();
        let mut bytes = 0;
        while let Some(row) = rows.next()? {
            let event = Event {
                id: row.get(0)?,
                revision: row.get(1)?,
                kind: row.get(2)?,
                text: row.get(3)?,
                request: row.get(4)?,
            };
            bytes += serde_json::to_vec(&event)?.len() + 1;
            if bytes > self.limits.context_bytes {
                return Err(Error::ContextUnavailable);
            }
            events.push(event);
        }
        if input
            .causal_events
            .iter()
            .any(|id| !events.iter().any(|e| e.id == *id))
            || !events
                .iter()
                .any(|e| e.kind == "call" && e.request == Some(receipt.request))
        {
            return Err(Error::ContextUnavailable);
        }
        let context = Context {
            agent: input.agent,
            session,
            request: receipt.request,
            revision,
            cancellation,
            instruction: input.instruction,
            payload: input.payload,
            outcome: outcome.ok_or(Error::ContextUnavailable)?,
            causal_events: input.causal_events,
            history: events,
        };
        if serde_json::to_vec(&context)?.len() > self.limits.context_bytes {
            return Err(Error::ContextUnavailable);
        }
        Ok(context)
    }
    pub fn admit_resume(&mut self, lease: &Lease, context: &Context) -> Result<()> {
        // Reconstruct trusted data; caller cannot substitute a different context body.
        let current = self.build_context(Receipt {
            request: context.request,
        })?;
        if serde_json::to_string(&current)? != serde_json::to_string(context)? {
            return Err(Error::RevisionChanged);
        }
        if context.session != lease.session {
            return Err(Error::Unauthorized);
        }
        let incarnation = self.incarnation;
        let hook = self.hook.clone();
        let (tx, now) = self.begin()?;
        fence(&tx, lease, incarnation, now)?;
        request_open(&tx, context.request, lease.session, now, "accepted")?;
        let (revision, cancellation) = session_state(&tx, lease.session)?;
        if revision != context.revision || cancellation != context.cancellation {
            return Err(Error::RevisionChanged);
        }
        let changed = tx.execute(
            "UPDATE jobs SET state='ready',context=? WHERE request=? AND state='pending'",
            params![serde_json::to_string(context)?, context.request],
        )?;
        if changed != 1 {
            return Err(Error::NotReady);
        }
        hook(Boundary::JobBeforeCommit);
        tx.commit()?;
        hook(Boundary::JobAfterCommit);
        Ok(())
    }
    pub fn claim_job(&mut self, lease: &Lease, receipt: Receipt) -> Result<JobAttempt> {
        let incarnation = self.incarnation;
        let cap = self.limits.job_attempts;
        let (tx, now) = self.begin()?;
        fence(&tx, lease, incarnation, now)?;
        request_open(&tx, receipt.request, lease.session, now, "accepted")?;
        // Recover stale attempts within a still-running authority after lease takeover.
        tx.execute("UPDATE jobs SET state='ready' WHERE request IN (SELECT id FROM requests WHERE session=?1) AND state='running' AND (incarnation!=?2 OR generation!=?3 OR worker!=?4)",params![lease.session.0,incarnation,lease.generation,lease.worker])?;
        let active:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM jobs j JOIN requests r ON r.id=j.request WHERE r.session=? AND j.state='running')",[lease.session.0],|r|r.get(0))?;
        if active {
            return Err(Error::NotReady);
        }
        let (state, attempts, context): (String, i64, Option<String>) = tx.query_row(
            "SELECT state,attempts,context FROM jobs WHERE request=?",
            [receipt.request],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        if state != "ready" {
            return Err(Error::NotReady);
        }
        if attempts >= cap {
            tx.execute(
                "UPDATE jobs SET state='exhausted' WHERE request=?",
                [receipt.request],
            )?;
            tx.execute(
                "UPDATE requests SET state='recovery_failed' WHERE id=?",
                [receipt.request],
            )?;
            quarantine(&tx, receipt.request, "execution_exhausted")?;
            tx.commit()?;
            return Err(Error::Capacity);
        }
        let context: Context = serde_json::from_str(&context.ok_or(Error::ContextUnavailable)?)?;
        // Each physical attempt must start from a current manifest, including on restart.
        let (revision, cancellation) = session_state(&tx, lease.session)?;
        if context.revision != revision || context.cancellation != cancellation {
            tx.execute(
                "UPDATE jobs SET state='pending',context=NULL WHERE request=?",
                [receipt.request],
            )?;
            tx.commit()?;
            return Err(Error::RevisionChanged);
        }
        let attempt = increment(attempts)?;
        tx.execute("UPDATE jobs SET state='running',attempts=?,incarnation=?,generation=?,worker=? WHERE request=?",params![attempt,incarnation,lease.generation,lease.worker,receipt.request])?;
        tx.commit()?;
        Ok(JobAttempt {
            request: receipt.request,
            attempt,
            lease: lease.clone(),
            context,
            output_budget: self.limits.payload_bytes,
        })
    }
    pub fn complete_job(&mut self, attempt: &JobAttempt, result: &str) -> Result<()> {
        bounded(result, self.limits.payload_bytes)?;
        let incarnation = self.incarnation;
        let hook = self.hook.clone();
        let (tx, now) = self.begin()?;
        fence(&tx, &attempt.lease, incarnation, now)?;
        request_open(&tx, attempt.request, attempt.lease.session, now, "accepted")?;
        let changed=tx.execute("UPDATE jobs SET state='done',result=? WHERE request=? AND state='running' AND attempts=? AND incarnation=? AND generation=? AND worker=?",params![result,attempt.request,attempt.attempt,incarnation,attempt.lease.generation,attempt.lease.worker])?;
        if changed != 1 {
            return Err(Error::NotReady);
        }
        // Append, never replace live history when it advanced during execution.
        append(
            &tx,
            attempt.lease.session,
            "result",
            result,
            Some(attempt.request),
        )?;
        tx.execute(
            "UPDATE requests SET state='done' WHERE id=?",
            [attempt.request],
        )?;
        release_pins(&tx, attempt.request)?;
        hook(Boundary::ResultBeforeCommit);
        tx.commit()?;
        hook(Boundary::ResultAfterCommit);
        Ok(())
    }
    /// Settle a failed physical attempt without waiting for lease turnover. The
    /// attempt fence prevents a delayed failure from closing a newer attempt.
    pub fn fail_job(&mut self, attempt: &JobAttempt, failure: HarnessFailure) -> Result<()> {
        let incarnation = self.incarnation;
        let (tx, now) = self.begin()?;
        fence(&tx, &attempt.lease, incarnation, now)?;
        request_open(&tx, attempt.request, attempt.lease.session, now, "accepted")?;
        let changed = tx.execute(
            "UPDATE jobs SET state='failed' WHERE request=? AND state='running' AND attempts=? AND incarnation=? AND generation=? AND worker=?",
            params![attempt.request, attempt.attempt, incarnation, attempt.lease.generation, attempt.lease.worker])?;
        if changed != 1 {
            return Err(Error::NotReady);
        }
        tx.execute(
            "UPDATE requests SET state='recovery_failed' WHERE id=?",
            [attempt.request],
        )?;
        let classification = match failure {
            HarnessFailure::OutputBudgetExceeded => "harness_output_budget",
            HarnessFailure::ExecutionFailed => "harness_execution_failed",
        };
        quarantine(&tx, attempt.request, classification)?;
        tx.commit()?;
        Ok(())
    }
    /// Explicit failure policy for missing/oversized context or unsupported recovery.
    pub fn fail_resume(&mut self, lease: &Lease, receipt: Receipt) -> Result<()> {
        let incarnation = self.incarnation;
        let (tx, now) = self.begin()?;
        fence(&tx, lease, incarnation, now)?;
        request_open(&tx, receipt.request, lease.session, now, "accepted")?;
        let changed = tx.execute(
            "UPDATE jobs SET state='failed' WHERE request=? AND state IN ('pending','ready')",
            [receipt.request],
        )?;
        if changed != 1 {
            return Err(Error::NotReady);
        }
        tx.execute(
            "UPDATE requests SET state='recovery_failed' WHERE id=?",
            [receipt.request],
        )?;
        quarantine(&tx, receipt.request, "context_unavailable")?;
        tx.commit()?;
        Ok(())
    }
    pub fn cancel_session(&mut self, session: SessionId) -> Result<()> {
        let (tx, _) = self.begin()?;
        let (_, generation) = session_state(&tx, session)?;
        tx.execute(
            "UPDATE sessions SET cancellation=? WHERE id=?",
            params![increment(generation)?, session.0],
        )?;
        let ids = Self::request_ids(
            &tx,
            "SELECT id FROM requests WHERE session=? AND state IN ('pending','accepted')",
            session.0,
        )?;
        for id in ids {
            Self::close(&tx, id, "cancelled")?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn cancel_request(&mut self, lease: &Lease, receipt: Receipt) -> Result<()> {
        let incarnation = self.incarnation;
        let (tx, now) = self.begin()?;
        fence(&tx, lease, incarnation, now)?;
        let source: i64 = tx.query_row(
            "SELECT session FROM requests WHERE id=?",
            [receipt.request],
            |r| r.get(0),
        )?;
        if source != lease.session.0 {
            return Err(Error::Unauthorized);
        }
        Self::close(&tx, receipt.request, "cancelled")?;
        tx.commit()?;
        Ok(())
    }
    fn request_ids(tx: &Connection, sql: &str, value: i64) -> Result<Vec<i64>> {
        let mut stmt = tx.prepare(sql)?;
        let rows = stmt.query_map([value], |r| r.get(0))?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }
    fn close(tx: &Connection, request: i64, state: &str) -> Result<()> {
        tx.execute(
            "UPDATE requests SET state=? WHERE id=? AND state IN ('pending','accepted')",
            params![state, request],
        )?;
        tx.execute(
            "UPDATE jobs SET state='cancelled' WHERE request=? AND state!='done'",
            [request],
        )?;
        tx.execute(
            "UPDATE outbox SET state='cancelled' WHERE request=? AND state='ready'",
            [request],
        )?;
        // A closed continuation does not turn an admitted external attempt into no effect.
        let unknown: bool = tx.query_row(
            "SELECT state='in_flight' FROM outbox WHERE request=?",
            [request],
            |r| r.get(0),
        )?;
        if unknown {
            quarantine(tx, request, "unknown")?;
            tx.execute(
                "UPDATE outbox SET state='unknown' WHERE request=?",
                [request],
            )?;
        }
        release_pins(tx, request)?;
        Ok(())
    }
    pub fn expire(&mut self) -> Result<usize> {
        let (tx, now) = self.begin()?;
        let ids = Self::request_ids(
            &tx,
            "SELECT id FROM requests WHERE deadline<=? AND state IN ('pending','accepted')",
            now,
        )?;
        for id in &ids {
            Self::close(&tx, *id, "expired")?;
        }
        tx.commit()?;
        Ok(ids.len())
    }
    /// Atomic local delivery: one recovery event per effect. No recursive notification DLQ.
    pub fn notify_recovery(&mut self, receipt: Receipt) -> Result<bool> {
        let hook = self.hook.clone();
        let (tx, _) = self.begin()?;
        let record: Option<(String, bool)> = tx
            .query_row(
                "SELECT classification,notified FROM recovery WHERE request=?",
                [receipt.request],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let Some((classification, notified)) = record else {
            return Err(Error::NotReady);
        };
        if notified {
            tx.commit()?;
            return Ok(false);
        }
        let source: i64 = tx.query_row(
            "SELECT session FROM requests WHERE id=?",
            [receipt.request],
            |r| r.get(0),
        )?;
        append(
            &tx,
            SessionId(source),
            "recovery",
            &classification,
            Some(receipt.request),
        )?;
        tx.execute(
            "UPDATE recovery SET notified=1 WHERE request=?",
            [receipt.request],
        )?;
        hook(Boundary::RecoveryBeforeCommit);
        tx.commit()?;
        hook(Boundary::RecoveryAfterCommit);
        Ok(true)
    }
    pub fn history(&self, session: SessionId) -> Result<Vec<Event>> {
        history(&self.conn, session)
    }
    pub fn request_state(&self, receipt: Receipt) -> Result<String> {
        Ok(self.conn.query_row(
            "SELECT state FROM requests WHERE id=?",
            [receipt.request],
            |r| r.get(0),
        )?)
    }
    pub fn outbox_state(&self, receipt: Receipt) -> Result<String> {
        Ok(self.conn.query_row(
            "SELECT state FROM outbox WHERE request=?",
            [receipt.request],
            |r| r.get(0),
        )?)
    }
    pub fn job_state(&self, receipt: Receipt) -> Result<String> {
        Ok(self.conn.query_row(
            "SELECT state FROM jobs WHERE request=?",
            [receipt.request],
            |r| r.get(0),
        )?)
    }
    /// Bounded durable queue scan; no eager reconstruction of an unbounded RAM queue.
    pub fn pending_outbox(&self, limit: usize) -> Result<Vec<Receipt>> {
        if limit > 64 {
            return Err(Error::Invalid("scan limit exceeds 64"));
        }
        Ok(Self::request_ids(
            &self.conn,
            "SELECT request FROM outbox WHERE state='ready' ORDER BY request LIMIT ?",
            limit as i64,
        )?
        .into_iter()
        .map(|request| Receipt { request })
        .collect())
    }
    pub fn pending_jobs(&self, limit: usize) -> Result<Vec<Receipt>> {
        if limit > 64 {
            return Err(Error::Invalid("scan limit exceeds 64"));
        }
        Ok(Self::request_ids(
            &self.conn,
            "SELECT request FROM jobs WHERE state IN ('pending','ready') ORDER BY request LIMIT ?",
            limit as i64,
        )?
        .into_iter()
        .map(|request| Receipt { request })
        .collect())
    }
}
