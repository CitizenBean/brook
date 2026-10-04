# Local admission, ownership, recovery and retention

This extends the [session and routing design](context-and-routing.md). It records agreed behavior and proposed mechanisms for a documentation-only platform. No storage engine or runtime implementation has been selected.

## Agreed direction

- Focus the first durable design on a **local transactional store**. Admission is recoverable before Brook acknowledges accepted work.
- A worker has a stable worker ID; a session has a separate ownership generation that increases when ownership is granted again. Heartbeats renew a lease. Another worker cannot take a valid lease from its owner.
- Unresolved/exhausted failures and uncertain external outcomes go to a dead-letter recovery path that can reach the originating agent session. This must not blindly resend an uncertain effect.
- Disk and RAM usage must be bounded. Cleanup cannot silently discard accepted work or required live state.
- Unused records have a **configurable seven-day retention default**. Still-used material is protected by an explicit live reference/pin. Releasing the final pin on an already old record makes it eligible for cleanup; incidental reads do not restart its age.

The record layouts, exact clock basis, resource units, age anchors, retry windows and policy APIs below are proposals. The seven-day unused-record default is not a seven-day automatic kill switch for active work.

## Durable admission and receipts

**Agreed acceptance boundary:** acknowledge `accepted` only after a recoverable bundle has committed: request identity and immutable arguments, source session and causal references, pending continuation for await-result work, and authorized outgoing intent. The durable record must also provide the same stable pending receipt on retry. Returning a receipt is acknowledgement of admission, not completion of the external work.

Before that commit, preparation may fail or the process may crash without accepted work. After it, a process crash or lost response must not lose the admitted request. Retrying the same scoped operation recovers its original receipt and logical work; it does not create another request or effect.

**Proposed local operation key:** authenticated namespace + resolved internal session + operation kind + caller operation ID. Treat caller IDs as scoped idempotency keys, not global authority. Bind the key to the immutable submission arguments, including destination and causal references. A retry with the same key and same arguments returns the original receipt. The same key with different arguments returns an explicit conflict and preserves the original record. A canonical digest may support comparison, but canonicalization and collision/error handling still need a concrete contract.

This retry guarantee has a declared validity window. Deduplication records and admission receipts are retained through that window. Once it closes, an old operation is rejected as stale; forgetting its receipt must not make it fresh work. The proposed age/sequence boundary is described under retention. An operation ID is not reusable merely because a process restarted or a record was collected.

The transaction is **per durable step**, not graph-wide. No LLM call, remote sink operation or long-running tool invocation belongs inside it. Admission coordinates the local records and budget reservations; dispatch runs after commit. Reply acceptance, resume admission and terminal transitions have their own local atomic boundaries. Their composition into a complete runtime still needs implementation design and tests.

On restart, reconstruct dispatch/resume queues from committed records, not from process memory or an unacknowledged receipt buffer. The durable outbox may drive another delivery attempt with the same identity. It does not itself prove exactly-once effects at a provider.

An explicitly ephemeral in-memory profile remains useful for tests. It must advertise that acceptance, receipts, deduplication and pending work can be lost on process failure. It cannot claim the durable profile's restart guarantee. The model includes a receipt → crash → lost-work example for this weaker profile.

## Local ownership and heartbeats

The proposed local ownership record contains:

```text
session ID + stable worker ID + persistent ownership generation + lease deadline
```

This is separate from the session's cancellation generation and context revision. Taking over execution must not cancel a valid durable continuation, discard its history, or rename its logical request. The new owner resumes that work with its newly granted ownership token.

The local transactional store arbitrates claim, renewal, release and protected commits. With authoritative time `now`, a lease is valid only when `now < deadline`; it is expired when `now >= deadline`.

- **Claim:** atomically compare the current owner/generation and eligibility. Grant only if unowned by explicit release or expired, and increment the ownership generation. Concurrent contenders cannot both win the same transition. A successful earlier preflight is not sufficient.
- **Heartbeat:** renew only the current worker and generation while the lease is still valid. A late heartbeat cannot revive an expired or superseded grant. The worker must obtain a new grant after expiry.
- **Release:** close the current valid grant. Reacquiring even with the same worker ID creates a new generation, preventing an old token from becoming valid again.
- **Protected commit:** validate current worker, generation and unexpired lease in the same authoritative transition that admits state/result/effect changes. A worker waking after a pause or takeover cannot commit with its old token.

These are local atomic storage contracts, not a promise that only one physical harness process can be running. A paused or partitioned worker might still execute instructions while its authority has expired; fencing prevents its later protected commits. Heartbeats establish lease authority, not proof of physical life or death. A live worker unable to renew can lose an expired lease. Lease duration, heartbeat interval and grace policy remain operational choices; they must not weaken the explicit validity test silently.

**Clock/restart boundary:** the bounded model assumes one monotonic authoritative clock and persistent generations. A process-local monotonic clock normally resets on restart and cannot simply be compared with an old persisted deadline. A boot epoch or another comparable authority-time policy must be selected before implementation. Ownership generations must not reset with worker memory. Handling restart or an unavailable clock/authority must preserve fencing; clock uncertainty is not permission to steal a potentially valid lease.

A future distributed profile could use a consensus-backed authoritative store for the same atomic interface. Store leader election is distinct from choosing a session's worker. This revision chooses no consensus algorithm and contains no distributed implementation or consensus model; the focus is the local store.

External in-flight sends remain a separate boundary. A newly expired lease cannot retract a provider request already admitted/sent. Stronger provider-side fencing requires provider cooperation; the earlier delivery-admission limitation remains in force.

## Dead-letter recovery reaches the agent

An external effect may be confirmed successful, confirmed not applied/failed, or **outcome unknown**. A timeout or lost response does not prove that an email was not sent. Preserve that distinction when quarantining an unresolved or exhausted attempt.

**Proposed recovery record:** source session/namespace, request and stable effect/intent identity, resolved destination/account, attempt identity/count, outcome classification, relevant evidence references, and recovery state/budget. Commit the dead-letter entry and its correlated recovery-notification intent durably. The notification goes to the originating agent session, not automatically to the failed external sink.

Deduplicate the logical recovery event by effect and failed/uncertain attempt. Retries of notification delivery must not produce multiple logical agent events for that episode. A later authorized attempt can have a new bounded episode while preserving the original effect identity. Notification attempts share a finite recovery budget; a failed notification must not create a fresh unbounded chain of dead letters about dead letters. Exhaustion leaves an inspectable record for an explicit operator/recovery policy; it does not prove that the agent will eventually receive the event.

The agent can inspect evidence, reconcile with an adapter, ask the user, or propose a recovery action. Its receipt of an uncertainty event does not itself authorize resending. A recovery retry needs the relevant policy/user authority and a safe adapter contract. For a non-idempotent uncertain effect, require authoritative evidence that it did not apply before retrying; otherwise keep it quarantined or seek a deliberate resolution. An adapter's genuine idempotency guarantee can permit an authorized retry under the same stable effect key. Never change the key to evade deduplication.

Confirmed failure means the adapter can establish that this attempt did not apply; ambiguous error responses stay unknown. Moving a record to the DLQ does not resolve that uncertainty or create an exactly-once guarantee. Reconciliation that confirms prior success should close the recovery without resending. Exact evidence schemas, operator escalation and adapter-specific reconciliation remain implementation policy.

## Bounded storage, pins and age

Age and capacity limits solve different problems. Seven-day retention does not prevent a busy system from filling its disk or RAM in minutes. Apply configured byte/count budgets as well as age eligibility.

**Proposed age anchors:** ordinary immutable event/causal records age from their creation; completed outcomes and dead-letter records age from their terminal/quarantine creation. Deduplication/tombstone retention must additionally cover the supported retry/replay window. These per-class anchors require review and are not hidden changes to the agreed seven-day default.

A record is “still used” when an explicit live reference needs it: a pending or accepted continuation, current state/snapshot, recovery process, or another declared dependency. A read or agent inspection alone does not extend retention. Current state is not evicted merely because its last update is older than seven days; retain its required snapshot and recovery tail until superseded safely.

Pin only the minimum necessary causal material. Pending work also needs its own configured deadline/storage budget; unlimited pins are not a capacity policy. Pending-work expiry is separate from unused-record retention. If a configured deadline expires, first commit an explicit terminal `expired` outcome, close the continuation and release its pins. Only then may cleanup reclaim now-eligible material. Do not delete a live request to make an occupancy invariant pass.

When the final reference is released, an already age-expired record is immediately **eligible** for collection. Actual collection still requires scheduling and crash-safe storage operations. Neither the design nor the safety model promises instantaneous or eventual cleanup without those progress assumptions.

**Proposed admission/capacity sequence:** estimate and reserve the new step's durable and volatile footprint; clean eligible material; then backpressure or explicitly reject if the budgets still cannot accommodate it. Never acknowledge accepted work and silently drop it later because a queue or disk filled. Reserve space for terminal outcomes, recovery/control records and cleanup so ordinary work cannot consume all capacity needed to complete or expire existing work.

Bound queues, payload sizes, fan-out, cached context, current/operator state, deduplication records and DLQ storage independently. Admission estimates must account for the actual engine's WAL, indexes, filesystem allocation, compaction/temporary copies and reserved headroom. The abstract model's units are not a measured disk/RAM bound, and no physical disk-full/fsync behavior is verified here.

**Proposed stale-operation rule:** use an authenticated/server-bound operation age or sequence floor within each admission scope. Keep the admission/tombstone for the whole retry window; after the floor advances, reject an older ID even if its record is gone. Caller-supplied timestamps must not defeat that boundary. Starting deliberately new work requires a fresh valid operation identity and authorization, not blind replay under a forgotten ID. Exact window lengths, persistence/compaction of the floor and replay policy remain open.

## Checked scope and next implementation work

The [TLA+ models and results](../spec/README.md) now include admission/crash recovery, local leases, bounded dead-letter recovery and retention/capacity. They include positive traces and mutations that expose unsafe alternatives. The existing session and sink checks remain separate models; there is no proof that the slices compose automatically.

Atomic local transactions, accurate accounting, trusted clock/identity/policy inputs, durable reference validity and adapter claims are assumptions. Models use small abstract time/resource units, not seven literal day ticks. They check safety and reachable examples without fairness or eventual-cleanup guarantees. Physical storage, provider effects, process crashes during real database recovery, clock restart policy and implementation refinement still require concrete design and tests. Agent-loop behavior is outside this revision.
