# Bounded architecture checks

These TLA+ models make a small part of the [architecture draft](../docs/architecture.md) executable for review. They are **bounded safety checks of proposed contracts**, not a proof of Brook, its future Rust implementation, a storage backend, or arbitrary system sizes. The current revision starts from `95718d201228680539cb8289fa0bf53fca77415d`; the initial wait/continuation slice was introduced against `24ff656d73626b8cf010022ca1216705ec4627a8`. The concrete [session/context/delivery proposal](../docs/context-and-routing.md) explains the intended contracts and their open implementation choices.

The wait graph contains outstanding **logical-work waits**, not routing connections or agent identities. This scope is a modelling assumption pending confirmation. Messages may return to an agent without adding a wait; fire-and-forget loops and their budgets are outside the model.

## Run

Install Java and Python 3 through your normal tooling. Download the official tools locally; nothing here needs a global installation or credentials:

```sh
curl -fL https://github.com/tlaplus/tlaplus/releases/download/v1.8.0/tla2tools.jar -o /tmp/tla2tools.jar
python3 spec/check.py /tmp/tla2tools.jar
# Optional: run only one configuration.
python3 spec/check.py /tmp/tla2tools.jar --case Continuations-overwrite
```

The runner invokes TLC breadth-first search with one worker, a 1 GiB Java heap, parallel GC, seed 1 and fingerprint polynomial 0. Each run uses a temporary directory, has a 180-second timeout, and saves a path-sanitized log in `results/`. A safe case passes only on successful completion with an empty queue. A negative control passes only when TLC returns failure with the specified invariant violation and a trace. A parser error, timeout, unrelated failure, or unexpectedly safe mutation fails the runner. Negative controls stop at the first counterexample; they do not exhaust their unsafe state spaces.

See [RESULTS.md](RESULTS.md) for the exact artifact hash, TLC version, bounds, state counts, and counterexamples. Release labels alone do not identify the tested binary; compare the printed SHA-256 when reproducing. [Official TLC source and tools](https://github.com/tlaplus/tlaplus) document the tool; the `.cfg` files are authoritative for each run's constants and invariant list.

## Wait admission

`Waits.tla` explores three work nodes and two attempts per edge. `Check` records a successful preflight, so several workers can check against the same graph. Safe `Commit` rechecks acyclicity and inserts the edge in one atomic transition. `Finish` removes an edge for completion, cancellation, or timeout. `Retry` increments an attempt while retaining the edge and logical identity. A new admission after `Finish` may reuse the pair and restart its bounded attempt count.

- `TypeOK`: edges, preflight proposals, attempts and identities stay in their domains.
- `NoWaitCycle`: no node reaches itself through any number of current wait edges; the finite closure covers indirect cycles as well as self-waits.
- `RetryIdentity`: retry attempts never rename their logical edge.

`Waits-racy.cfg` skips commit-time revalidation, exposing concurrent check/insert races. `Waits-retry.cfg` changes identity on retry. Check/insert atomicity is a **required coordination contract**, not a demonstrated lock, transaction, distributed consensus, or fencing implementation. Logical nodes are assumed stable and globally meaningful within the graph. One edge per ordered pair suffices here; multiple separately cancellable waits between the same pair would require reference counting or a richer representation.

## Continuations

`Continuations.tla` explores two distinct requests in one session, context revisions 0–2, cancellation generations 0–1, and at most two outbound dispatch attempts per request. All operations interleave, including reply handling for two requests, newer session activity, retries, timeout/closure, cancellation and resume.

`Prepare` atomically persists a checkpoint reference, request record and outgoing intent, capturing separate context revision and cancellation generation. `Dispatch` requires those records; `Retry` preserves effect identity. `Accept` verifies correlation, reply shape, captured generation, current generation and pending status, then atomically records the reply, claim and resume intent. `Resume` rechecks cancellation generation, consumes the logical continuation once, inserts its logical call/outcome records and reconstructs context from durable activity plus the checkpoint's revision range. The immediate provider pending receipt and provider transcript formatting are abstracted, not modelled as a delayed second tool result.

`Advance` adds a concrete activity item, not just a version counter. `Cancel` changes generation without changing revision. Newer context alone does not reject a valid reply: `Continuations-advancedWitness.cfg` deliberately asserts that such acceptance never happens and requires TLC to refute it. This is a reachability witness, not a broken safety contract.

The durable state is the checkpoint/request/outbox records, captured metadata, status, reply/resume intents, session activity and logical materialization records (`calls`, `results`, `runs`). `context` is a disposable projection. `Evict` models cache loss, including a crash that loses that projection; reconstruction uses durable activity. Counters, validity flags, `sent`, and `advancedReply` are observation/history variables. No volatile reply-claim state is necessary in the safe model because acceptance is one durable transition. In the split-accept mutation the reply is durable without its resume intent: a crash or indefinite stutter at that point cannot recover a resume from the model's stored intents.

Replies are chosen nondeterministically from sent requests with arbitrary correlation, generation and shape. Old-generation replies remain possible after cancellation; there is no FIFO queue assumption. Wrong correlation/shape, closed, stale and duplicate deliveries are disabled acceptance actions and can be ignored via stuttering. Delivery may repeat indefinitely. There is no fresh logical request ID for a retry. The two original request IDs are never reused within this model.

| Invariant | What it checks |
| --- | --- |
| `TypeOK` | Record domains, finite revision/generation/attempt bounds, set and counter types |
| `RecoveryBeforeDispatch` | Every observed dispatch has checkpoint, request and outbox records |
| `AtomicAcceptance` | Every recorded reply has a durable resume intent, and conversely |
| `ValidAcceptance` | Every acceptance respected pending status, correlation and generation |
| `ValidResume` | Every logical resume used the current cancellation generation |
| `AtMostOneClaim`, `AtMostOneResume` | Per-request counters remain at most one; set union cannot hide duplicates |
| `PreserveNewerActivity` | Every resume materialization contains all durable session activity existing at that transition |
| `StableEffectIdentity` | Retries preserve request/effect identity |
| `ToolCallResultPairing` | Materialized tool calls and results have matching request identities |

The mutation configurations bypass these contracts one at a time. The duplicate-resume case intentionally checks only `TypeOK` and `AtMostOneResume` so the earlier double-claim violation does not mask an actual second resume.

## Trusted session routing

`Routing.tla` has two authenticated namespaces, two trusted client instances per namespace and one external conversation ID (`42`) reused by all four keys. Its registry maps each key to a distinct internal session. Two immutable request records point from chat source sessions to separate agent/service destination sessions. Returns use the stored source and expected authenticated responder. This model supplies the routing boundary for the separate session/context model; there is no proved composition between them.

`RegistryIsInjective`, `TrustedIngress`, `ReturnToOrigin` and `AuthenticatedReply` check nonaliasing, trusted ingress resolution, return ownership and responder binding. Mutations omit namespace/client, trust a claimed payload destination, send a return to the work destination, or skip responder validation. The spoof witness shows a forged claimed session ignored in favor of trusted metadata. Authentication itself, registry creation/migration, deliberate aliases, request lookup failures and capability cryptography are abstracted. Registry aliases are intentionally unsupported by this configuration.

## Multiple sessions and retained context

`SessionContext.tla` has two sessions, three requests (`a1`, `a2` from `sA`; `b1` from `sB`), one optional fresh message per session, a single session cancellation generation change, and finite tagged history items. `a1` is issued before `a2`; replies and resumptions are otherwise unordered. Every item carries its owning session identity. Bounds imply at most six history items in `sA` and four in `sB`; there is no production token-budget algorithm here.

`Prepare` fixes the request's source, captures causal history references including its logical call, and records the current source generation. `Accept` appends that request's correlated outcome only to its owner and atomically records a claim and resume intent. This slice abstracts dispatch/pending receipt after preparation; the original `Continuations` slice checks the outbox prerequisite. A result item means a logical continuation outcome, **not** a provider-specific delayed tool result. The portable design completes the dispatch provider tool call with a pending receipt and handles this later item as fresh continuation input.

`history` is an append-only set of immutable identity references within the bounds. `revision` counts only that session's appended items. `cache` selects full, compact or evicted projection: a compact projection no longer displays original calls; a summary's semantic content is not modelled. `captured` retains request-specific reference sets independently of the projection. `archiveAvailable` abstracts recoverability of their original contents at session granularity. Normal `Reclaim` is forbidden while any request in that session is pending or accepted. `LoseArchive` is a separate environmental storage-loss fault; it deliberately can defeat a pin. A reply with unavailable original material reaches `RecoveryFailed`, not successful `Resume`.

`Resume` computes a candidate reconstruction from current source history and the captured references. Observation flags check that **each materialization event** contains its causal call/outcome and all newer bounded history, uses recoverable contents, and contains no other-session items. These are safety monitors over the candidate reconstruction, not claims that the compacted projection is lossless or that a real prompt contains every history item. Current history/projection is never replaced by the old checkpoint. `cache` changes do not mutate history or generation.

`Cancel` changes one session's generation and closes its active requests. `CloseRequest` closes one request without incrementing session generation. Immutable owner functions, actual session-indexed history/revision/generation, bounded claim/run counters and cumulative materialization monitors are all included in `TypeOK`.

| Invariant | Contract |
| --- | --- |
| `HistoryIsolation`, `CapturedIsolation` | History and retained causal references belong to their source session |
| `ContextIsolation` | Every reconstructed context contains only source-session items |
| `SessionLocalRevision`, `SessionLocalCancellation` | Another session's activity/cancellation cannot advance these counters |
| `RequestLocalClosure` | A request closes only from its own closure or source-session generation change |
| `OwnedReturn` | The correlated outcome is appended to the immutable source owner |
| `RetainedWhilePending` | Normal reclamation never removes active continuation material |
| `NoFabricatedRecovery` | Successful resume never uses unavailable original contents |
| `PreserveCausalAndNewer` | Reconstruction includes captured causal items, outcome and newer source history |
| `AtomicResumeIntent` | Each accepted request has its durable logical resume intent |

Mutations cover cross-session return/context, checkpoint rollback, global revision/cancellation, closing other requests, dropping pinned material and fabricated recovery. Positive witnesses require out-of-order **acceptance and resume** of `a2` before `a1` while `b1` is also active, eventual logical materialization of all three, resume in `sB` after cancellation in `sA`, reconstruction after compaction hid the call, and explicit failure after storage loss. Witnesses are reachable executions, not liveness guarantees. The normal safe configuration explores all action interleavings, including eviction, fresh messages and failures.

## Per-intent authorized sink admission

`Delivery.tla` has two intents from one chat-origin session and four endpoint identities: chat/email in each of two namespaces. An endpoint stands for a resolved provider, sending account, recipient/thread and namespace tuple. The authoritative allowed destinations for the source are its own chat and email endpoints. Thus the model allows an explicitly authorized cross-channel email; the original reply channel does not constrain every outgoing intent.

`Authorize` freezes the selected endpoint and binding version in the intent. `Check` is a potentially stale preflight. `Revoke` and `Rotate` can invalidate authority or reassign a route identifier between preflight and delivery. Safe `Admit` revalidates current authorization and uses the immutable intent target. It permits up to two attempts per intent with the same binding, without claiming exactly-once external effects. Version/serial values are 0–1 and do not wrap in the safe model. `serial` is an observation of binding incarnation; the reuse-version mutant exposes an old intent admitted against a new incarnation with a recycled externally checked version.

`AuthorizedIntent`, `ImmutableDestination` and `AuthorizedAtAdmission` separately check initial permission, unchanged per-intent destination and valid authority at each admission. Cumulative monitors retain what was true at the admission transition; later revocation does not incorrectly invalidate earlier history. Mutations allow unauthorized creation, payload retargeting, late route lookup, stale preflight admission and binding-version reuse. The late-lookup configuration intentionally omits `AuthorizedAtAdmission` so a prior authorization error does not mask the concrete retargeted endpoint. Positive witnesses require email admission and two different authorized destinations from the same source session.

This is a **delivery-admission** model, not a provider-send protocol. Validation and admission are one atomic abstract transition. Irreversible external delivery, provider failures, in-flight revocation, authorization policy evaluation, resolved-recipient correctness, content/data scopes, credential handling and provider-side fencing are not verified. Invalid admission is disabled (rejection/stutter); error reporting and eventual delivery are not proven. The security property is destination integrity under the trusted boundary, not comprehensive confidentiality or application security.

## Assumptions and limits

The models require atomic storage operations at admission, preparation, reply acceptance and logical resume materialization. Their purpose is to explain why these boundaries matter and expose alternatives that break them. A future implementation needs a refinement argument or tests connecting real transactions, crashes and concurrency to these abstract actions. No storage protocol has been chosen or verified. The five separate models do not establish a composition theorem between wait cleanup, continuation completion, authenticated ingress and authorized egress.

No fairness is assumed and no liveness property is checked. Stuttering, permanent transport loss and indefinitely postponed processing are allowed. Deadlock checking is disabled because bounded terminal states and quiescence are legitimate. Eventual completion would need explicit delivery, scheduler, recovery and cancellation assumptions. The advanced-reply witness demonstrates one possible execution, not inevitable progress.

The continuation models check at-most-once **logical** claim/materialization. It does not prove exactly-once physical harness invocation, tool effects or sink delivery. Cancellation fences later acceptance/resume; it does not undo a dispatch or an external effect. A cancellation after a valid materialization does not retroactively make that transition invalid.

Context items abstract immutable session activity; the selected policy reconstructs/merges all bounded items. Real context selection, summary content/quality, branching, event ordering, schemas, harness versions, authorization-policy evaluation, retention storage mechanisms and payload semantics are not modelled. The new slices check only the finite routing, pinning and projection abstractions described above. Matching logical call/outcome identities establish causal association only, not valid provider-specific transcript ordering or content. In the proposed portable protocol, the provider call is completed by a pending receipt and the outcome becomes fresh continuation input. The checkpoint is a portable reconstruction record; no arbitrary runtime internals are serialized, and no particular harness is required. Harness capability negotiation and unavailable-history behavior remain open.

Bounded completion gives evidence for precisely these configurations. It is not an inductive proof over unbounded nodes, requests, generations, history or attempts. TLC uses fingerprints; its reported collision estimates appear in the safe-run logs. Increasing bounds, adding implementation detail, and checking cross-model integration are future work.
