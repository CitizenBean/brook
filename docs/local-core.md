# Experimental local Rust core

The [processor slice](processors.md) extends this baseline with separate processing tables and a transactional schema 1→2 migration. Its DAG/terminal contracts do not replace the session continuation contracts below. Request retry equality now decodes the stored Submission; formatting differences are not semantic conflicts.

This first implementation is based on architecture draft `61e2ada58d1b4d2c7beefcace8bc4b9d2ed45ed6`. It exercises a local awaited-request lifecycle with SQLite, two interchangeable deterministic harnesses and a fake sink. It does not settle the broader graph, provider, authentication or distributed architecture.

## Run and verify

Requires Rust 1.89 or later and a native C compiler for bundled SQLite. Dependencies are pinned in `Cargo.lock`.

```sh
cargo run --locked -- demo /tmp/brook-demo-new
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

The demo requires a new directory and refuses to append into an existing Brook database. It creates two isolated sessions, issues requests A and B, appends newer activity, completes B, closes/reopens the store, recovers A's original receipt, and completes A using another harness. Both results and newer activity remain; the other session's history is unchanged. The fake sink has no network or external effects.

## Authority, ownership and time

One authority owns one canonical store directory. An OS advisory exclusive lock on `authority.lock` is held for the store's lifetime, including database shutdown. A second authority fails while that lock is held. Workers use the authority's API, serialized with `&mut Store` (or a host mutex); they never open their own database connections for writes. All mutations use immediate SQLite transactions, with foreign keys enabled, WAL journaling and `synchronous=FULL`.

On open, the authority transactionally increments a persistent incarnation, clears session lease owners, quarantines unresolved sends, and makes running jobs recoverable. Tokens bind the database's random persistent identity, authority incarnation, session, stable worker ID and persistent ownership generation. A successful claim increments the generation, including same-worker reacquisition. Heartbeat, release and protected writes validate the token and `now < deadline` in their transaction. Takeover never increments cancellation generation. A paused worker can still execute locally but cannot commit with its stale token.

The lock is advisory and the filesystem is trusted: no outside writers, lock-file unlink/replacement, live directory copying, alternate hard-link paths, or network filesystem deployment are supported. The lock descriptor must not be passed to child workers. The implementation relies on Rust's ordinary close-on-exec file handling, rather than launching/forking workers itself. Corrupt or malicious local files are not a security boundary.

Lease time is monotonic elapsed time within an authority incarnation. For request/grant deadlines, the store uses its last committed logical millisecond tick plus elapsed time since the supplied clock was created. Downtime, and elapsed time not committed before a crash, do not advance these deadlines. This is deliberately not wall-clock expiry. Restart invalidates leases by incarnation instead of comparing deadlines from different process clocks. Regressing time fails closed; integer exhaustion does not wrap. The default uses `Instant`; tests inject `ManualClock`. A future wall-clock retention or credential policy needs its own explicit restart/uncertainty contract.

## Trusted boundaries and immutable admission

These are trusted local host APIs, not remotely authenticated endpoints. The host supplies authenticated namespace/client/conversation mappings, responder identity and explicit policy grants. No model output is treated as an authorization grant. Sessions use a unique `(namespace, client, external conversation ID)` mapping and never reuse internal IDs.

This slice has one operation kind, await-result. Its deduplication key is `(resolved session, caller operation ID)`; the session already binds the namespace. The original schema-1 canonical submission stores JSON encoding of a fixed-field Rust structure; schema-2 retry comparison decodes its semantic fields without rewriting the old record. It contains no arbitrary maps or floating-point values. Text is compared byte-for-byte without Unicode normalization; ordered causal IDs must be sorted and unique. The agent configuration, instruction, payload, destination, grant/version, expected peer, causal references and lifetime are immutable. Any difference under the same key conflicts. Equality uses the full canonical contents, not a hash alone.

Admission atomically checks ownership, scoped uniqueness, policy and causal ownership; reserves a request slot; appends the logical call; pins causal records; and inserts the pending request and outbox. Only a successful commit returns a receipt. Matching retries recover that receipt even after completion, cancellation or policy revocation; they do not create or reauthorize an effect. Deduplication has no retirement window in this first slice: records are kept until the store is explicitly retired, and operation IDs are never reusable in that store.

A policy grant binds one session, fake sink, account, recipient and exact outgoing payload. Replacement increments its version; revocation leaves the record in place. Delivery admission rechecks that binding, version, activity, logical expiry and current ownership in the same transaction that creates the send attempt. There is no late recipient lookup or fallback route. Revocation after admission cannot undo an already admitted attempt.

## Effects, replies and logical jobs

An outbox starts `ready`. `begin_send` durably marks it `in_flight` before exposing the immutable adapter input. There is at most one physical send attempt in this slice. A confirmed applied result records `sent`; confirmed non-application records `failed`; ambiguity records `unknown`. Failure and unknown outcomes retain a recovery record. Authority restart or session takeover turns unresolved `in_flight` attempts into `unknown`, even if the process actually died just before invoking the sink. Neither a stable effect ID nor an authenticated reply alone authorizes a resend.

There is no automatic or manual redrive API yet. Unknown work remains inspectable and cannot return to `ready`. Reconciliation, adapter idempotency contracts and safe authorized retry are later work. A late reply can still resume its original pending continuation, but does not automatically resolve a separate unknown delivery record.

Replies use a single schema here: nonempty UTF-8 text within the payload limit. The trusted ingress peer must match the stored responder. The request must have reached send admission, remain pending and unexpired, and match the session cancellation generation. Acceptance atomically appends the correlated outcome and creates one durable pending job. Identical duplicate replies do nothing; conflicting replies fail. The reply API accepts no caller-selected destination session.

`build_context` streams current session events up to the context byte budget and checks retained causal IDs and the original logical call. It includes the pending receipt's request identity and outcome as fresh input. It does not insert a late provider tool result into an old transcript. No lossy summary is used in this slice. Missing/oversized context returns `ContextUnavailable`; the scheduler can record `recovery_failed` through `fail_resume`. It must not silently truncate. Pins remain available for recovery.

`admit_resume` checks the supplied context against a trusted reconstruction, then checks revision, cancellation, ownership and lifetime again in its transaction. It persists a bounded context manifest in the existing uniquely keyed job. A job is not completed merely because this manifest exists. `claim_job` admits one running job per session, increments a bounded physical attempt count and records its ownership fence. It requires a current manifest; newer activity resets the job to pending for explicit reconstruction. Different sessions can run independently. No await-result request holds a running-job slot while waiting.

Restart and takeover recover unfinished attempts using the same logical job identity. `complete_job` fences the physical attempt and atomically records its result, appends history and completes the request. History arriving during execution is preserved; results append rather than overwrite. Physical harness invocation may repeat after a crash. Only successful logical result commitment is deduplicated. Attempt exhaustion records explicit recovery failure. A running attempt can also be settled through fenced `fail_job`: it records a typed failure and recovery intent, then releases the active session slot without lease turnover. Late success/failure from that attempt cannot close newer work. The host schedules these operations; there is no background daemon or liveness guarantee.

Cancellation closes only the selected request or increments only the selected session's cancellation generation. It fences later reply/resume/result acceptance. Expiry is a separate explicit operation. Neither can retract prior effects. In-flight effects become unknown, and their recovery references remain protected.

## Harnesses and agent settings

`Harness` defines portable fresh-run execution independently of `AgentConfig`. Each immutable request snapshot contains an agent ID/version, role, instructions, model name, sampling/output settings and tool permissions. Two configurations can use the same harness; two harness implementations can consume the same portable context. The tests exercise both relationships. The harness receives the attempt's output byte budget and returns either a bounded string or a typed failure. The bundled harnesses omit optional prefixes when needed to preserve a maximum-sized outcome in full. The host must call `fail_job` on a harness failure; storage independently rejects oversized output from a misbehaving adapter. The fake implementations do not call models or execute tools, and model settings are inert metadata in this slice. Nonempty tool permissions are explicitly unsupported.

A future handoff can select another agent configuration while retaining the same harness implementation. It must specify target context, session/work scope and independently authorized capabilities. It must not silently rewrite the source agent's settings. Transfer versus request-and-return are possible future modes, not settled contracts. Nested dispatch/handoff, arbitrary graphs and wait-cycle enforcement are not implemented here.

## Capacity, recovery and retention limits

Limits are validated and persisted at creation; reopening with different limits is rejected until a migration/configuration-update protocol exists. Separate quotas bound sessions, policy grants, requests, ordinary messages per session, field/payload bytes, serialized context bytes and physical job attempts. Queue scans return at most 64 entries. Context construction stops when its serialized event budget is exceeded. Public history inspection is bounded by stored event-count/payload quotas, not by the smaller prompt budget.

Each request permanently consumes one admission slot with a conservative logical byte reservation: `context_bytes + 32 * payload_bytes + 65536`. The allowance includes JSON escaping, immutable inputs, at most four associated history events (call, outcome, result, recovery), bounded pins, output and recovery metadata. Those records do not compete with new-work admission or ordinary-message quotas. Payload/context limits still apply to completions. Reservations are not released in this slice, and there is no claim that this formula measures physical SQLite bytes.

A recovery row is also the durable pending-notification intent. `notify_recovery` atomically appends one source-session recovery event and marks it notified. Repeating it cannot create another event. It is an in-database notification, with no remote delivery attempts, recursive DLQ or replenishing retry budget. The first recovery classification for an effect is retained; richer multi-episode recovery is deferred. SQL failure leaves the same intent pending. Unknown/recovery-failed work retains its pins even after continuation closure. Pin release requires a terminal continuation, no live job, a settled successful/cancelled delivery, and no recovery dependency. A reply and job may complete while delivery is still in flight; their pins stay live through late uncertainty or restart quarantine. Delivery confirmation reevaluates release, so a later success can release pins once all other dependencies have ended.

**Configurable seven-day unused retention is not implemented.** The configuration records its intended default, but there is no collector and it does not delete data. All history, deduplication records and receipts remain under hard logical quotas. This deliberately favors rejection at capacity over incomplete stale-ID retirement or loss of live references. Before collection is enabled, implement authenticated operation retirement, age anchors, shared live references, crash-safe deletion and restart-safe age accounting.

SQLite WAL, page allocation, filesystem metadata, indexes, allocator overhead and temporary copies are outside the logical reservation calculation. WAL autocheckpoint/cache settings are operational controls, not a strict total disk/RSS proof. Actual disk-full, failed fsync, sudden power loss, corruption and storage-device honesty have not been tested. A transaction can fail for lack of physical capacity despite reserved logical headroom; no receipt is issued for a failed admission. Hardware failure can defeat the local durability assumptions. Real provider effects, OAuth, untrusted harness isolation/time limits, automatic scheduling and eventual cleanup remain unimplemented.

## Evidence

`tests/core.rs` exercises transactions through public APIs, including concurrency, isolation, immutable retry comparison, per-intent grants, lease/cancellation fences, pins, bounded attempts, context revision races and quota headroom. `tests/crash.rs` launches child test processes, waits at deterministic boundaries and kills them without destructors, then reopens the same database. Its 18 cases cover before/after admission, send admission, reply acceptance, job materialization, result commit, quarantine and notification, plus an applied fake-provider effect, a job claimed but not yet invoked, interruption from inside a harness invocation, and job completion while delivery remains in flight. The ignored `crash_child` test is a helper launched explicitly by the parent test, not missing coverage.

The focused [LocalExecution model](../spec/LOCAL-EXECUTION-RESULTS.md) checks bounded send/job restart transitions with negative controls. It assumes atomic durable records and trusted inputs, does not execute the Rust code, and does not prove refinement or composition of all earlier models.
