# Bounded architecture checks

These TLA+ models make a small part of the [architecture draft](../docs/architecture.md) executable for review. They are **bounded safety checks of proposed contracts**, not a proof of Brook, its future Rust implementation, a storage backend, or arbitrary system sizes. The source baseline is `24ff656d73626b8cf010022ca1216705ec4627a8`.

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

`Prepare` atomically persists a checkpoint reference, request record and outgoing intent, capturing separate context revision and cancellation generation. `Dispatch` requires those records; `Retry` preserves effect identity. `Accept` verifies correlation, reply shape, captured generation, current generation and pending status, then atomically records the reply, claim and resume intent. `Resume` rechecks cancellation generation, consumes the logical continuation once, inserts its tool-call/result pair and reconstructs context from durable activity plus the checkpoint's revision range.

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

## Assumptions and limits

The models require atomic storage operations at admission, preparation, reply acceptance and logical resume materialization. Their purpose is to explain why these boundaries matter and expose alternatives that break them. A future implementation needs a refinement argument or tests connecting real transactions, crashes and concurrency to these abstract actions. No storage protocol has been chosen or verified. Separate models do not establish a composition theorem between wait cleanup and continuation completion.

No fairness is assumed and no liveness property is checked. Stuttering, permanent transport loss and indefinitely postponed processing are allowed. Deadlock checking is disabled because bounded terminal states and quiescence are legitimate. Eventual completion would need explicit delivery, scheduler, recovery and cancellation assumptions. The advanced-reply witness demonstrates one possible execution, not inevitable progress.

The model checks at-most-once **logical** claim/materialization. It does not prove exactly-once physical harness invocation, tool effects or sink delivery. Cancellation fences later acceptance/resume; it does not undo a dispatch or an external effect. A cancellation after a valid materialization does not retroactively make that transition invalid.

Context items abstract immutable session activity; the selected policy reconstructs/merges all bounded items. Real context selection, compaction, branching, message ordering, schemas, harness versions, authorization, retention and payload semantics are not modelled. Matching tool-call/result identities establish pairing only, not valid provider-specific transcript ordering or content. The checkpoint is a portable reconstruction record; no arbitrary runtime internals are serialized, and no particular harness is required. Harness capability negotiation and unavailable-history behavior remain open.

Bounded completion gives evidence for precisely these configurations. It is not an inductive proof over unbounded nodes, requests, generations, history or attempts. TLC uses fingerprints; its reported collision estimates appear in the safe-run logs. Increasing bounds, adding implementation detail, and checking cross-model integration are future work.
