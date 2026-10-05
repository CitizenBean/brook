# Local durability, ownership and recovery

All graph deliveries, agent invocations, awaited work and result returns share
these contracts. Start with a local transactional authority; remote effects run
outside its transactions. The [architecture status](architecture.md#status-and-limits)
describes implementation gaps. [Session and routing rules](context-and-routing.md)
define trusted identities and per-intent authority.

## Atomic admission and deduplication

Acknowledge acceptance only after the step's recoverable records and capacity
reservations commit. For awaited work these include immutable arguments, source
causal references, correlation/result-return binding and authorized outbox intent.
The same records must reconstruct the receipt after a lost response or restart.
An admission acknowledgement is not an end-to-end protocol acknowledgement of
processing or delivery; connectors must specify those separately.

Scope caller operation IDs by authenticated namespace, resolved session/work scope
and operation kind. The same key and canonical immutable arguments return the
same logical receipt. Changed arguments conflict without altering the first
submission. Checking uniqueness, reserving capacity and inserting records are
one transaction, not a preflight followed by an unprotected write.

A transaction covers one durable step. Successful processor commit atomically
writes proposed state, the complete output/fan-out set and input completion.
Reply acceptance atomically records the correlated outcome and origin delivery.
Invocation admission validates current context/cancellation. No model execution,
network I/O or long-running callback occurs inside these transactions.

Reconstruct pending dispatch, result delivery and recovery from committed records,
not process memory. Repeated evaluation is allowed; generation/revision checks and
logical deduplication prevent multiple accepted results. No accepted work may be
silently lost because the process failed after commit or the receipt was lost.

Declare a retry window and preserve receipts/tombstones throughout it. Collection
must not make a forgotten old operation fresh. A server-bound age/sequence floor
can reject stale operations after their receipt is collected; arbitrary caller
timestamps cannot reset that floor. Exact window and floor representation remain
to be selected. Deliberately new work requires fresh identity and authority.

Ephemeral in-memory transport can lose receipts and pending events. It must expose
that weaker guarantee and cannot masquerade as durable admission. A future Kafka
backend still needs coordinated consumption, state and outbox recovery through
the shared execution contract.

## Heartbeat leases and fenced ownership

An ownership record contains the execution scope, stable worker ID, persistent
generation and authoritative lease deadline. Session invocation and node-state
scope rules must prevent conflicting accepted updates while allowing independent
sessions/keys to proceed. Ownership generation is separate from cancellation
and context revision.

With authoritative time `now`, validity means `now < deadline`:

- Claim only an explicitly unowned or expired scope, atomically incrementing its
  generation. Concurrent contenders cannot both win.
- Renew only the current worker/generation while valid. A late heartbeat cannot
  revive an expired grant.
- Release the current grant; reacquisition by the same worker still increments
  generation so old tokens stay stale.
- Check worker, generation, deadline and applicable state revision in the same
  transaction as every protected state/result/effect-admission commit.

Takeover preserves pending work and causal evidence. It does not cancel a valid
continuation or rename its effect. A paused worker can still execute physically
after losing authority; fencing rejects its later commits. A heartbeat is an
authority renewal, not proof of physical life or death.

Persist generations across restarts. A process-local monotonic clock cannot be
compared naively with a deadline from a previous boot. The authority needs a
restart-safe clock/epoch contract; uncertainty cannot authorize taking a
potentially valid lease. The production operational clock contract still needs validation.

A future distributed profile needs an authoritative atomic ownership service;
leader election for that service is distinct from assigning work to a worker.
No distributed consensus algorithm is selected. Neither local nor distributed
fencing can retract a provider request already admitted.

## Recover uncertain effects

Keep three external outcomes distinct: applied, confirmed not applied, and
unknown. A timeout or lost response does not establish non-application. An outbox
alone cannot guarantee exactly-once effects at a destination without suitable
idempotency or reconciliation.

Quarantine exhausted or uncertain attempts with source scope, request/effect ID,
pinned destination/account, attempt count, evidence and bounded recovery state.
Atomically record a correlated recovery-notification intent. For agent-originated
work, its destination is the originating agent session, not the failed external
sink; other work needs an explicit recovery route or inspectable status.

Deduplicate notifications by effect and failure episode. Bound notification
attempts so a failed notice cannot create an endless chain of notices about
notices. Exhaustion remains inspectable; it does not imply eventual notification.
Required evidence stays protected until recovery obligations settle.

A recovery event is evidence, not resend authority. The agent or operator may
inspect, ask the user or reconcile with the adapter. Retry an uncertain effect
only with relevant authority and a safe adapter contract: authoritative evidence
of non-application, or genuine idempotency under the same stable effect key.
Never rotate the key to evade deduplication. Confirmation of prior success closes
recovery without sending again. Otherwise preserve uncertainty; a deliberately
new action must explicitly account for the possible prior effect.

Confirmed failure requires evidence that the attempt did not apply. Ambiguous
provider errors remain unknown. Invocation cancellation, pipeline drain, moving
work to a dead-letter record and lease expiry do not resolve that uncertainty.

## Bounded resources and retention

Apply byte/count quotas as well as age limits. Seven days of retention alone can
still fill disk in minutes. Bound queues, pending work, concurrent evaluations,
input/output/state sizes, fan-out, cached context, retries, deduplication,
recovery records and telemetry. Native callbacks also need an honest isolation
or cooperative execution policy; a trait does not enforce CPU/memory limits.

Reserve capacity before admission, including headroom for completion, failure,
recovery and cleanup. Collect eligible records, then backpressure or reject new
work if it still cannot fit. Do not acknowledge work and later drop it to satisfy
a quota. Accounting must cover WAL, indexes, allocation overhead and temporary
copies, not just payload bytes.

Unused-record retention defaults to **seven days, configurable**. Proposed
initial age anchors are creation for ordinary immutable records and terminal/
quarantine creation for outcome/recovery records; these per-class choices remain
under review. Deduplication additionally covers its retry window.
Incidental reads do not reset age. The exact per-class policy must be exposed
and tested, not inferred from a generic last-access timestamp.

Live references protect pending and accepted continuations, current state and
its recovery tail, undelivered outcomes, unresolved recovery and pinned graph/
code/config/binding versions. A current state value is not evicted because its
last update is old. Pin the minimum necessary material, with finite pending-work
budgets and explicit deadlines.

Expiry of active work is separate from retention: commit its terminal outcome
first, settle dependent execution/effect/recovery obligations, then release only
references no longer needed. Cancellation or a received reply alone is not proof
that every pin can be released. After the final reference is removed, an already
old record becomes eligible for crash-safe collection. Eligibility does not
promise immediate or eventual cleanup without a progressing collector.

Compaction only changes historical representation. It neither deletes raw
protected evidence nor releases references. Retention may collect unreferenced
old history; context assembly must therefore select available evidence rather
than require an entire lifetime transcript.

## Verification boundaries

The preserved [TLA+ models and results](../spec/README.md) check bounded admission,
lease, recovery and retention abstractions. Atomic transactions, reliable
identity/clock inputs and accurate resource accounting are model assumptions.
The separate models do not prove automatic composition or physical disk-full,
fsync, provider or native-extension behavior.

Implementation gates include crash/receipt-loss admission, conflicting duplicate
submissions, concurrent claims, late heartbeats/results, restart fencing, atomic
fan-out, out-of-order returns with newer context, cross-session isolation,
revocation at effect admission, ambiguous sends without resend, recovery-budget
exhaustion and live-reference-safe collection under pressure. Retain existing
model artifacts while testing the unified implementation against these contracts.
