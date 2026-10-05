# Deterministic host simulations

This testing slice starts from published processor PR3, commit `c348cf4694b81ce7a72bb817e1f2c61b4289dd5b`. It adds tests and a replayable example around the **actual** Store, Processor, Harness and terminal sink APIs. It adds no production runtime, agent loop, provider integration or graph-to-agent bridge.

## Run and replay

```sh
cargo test --locked --offline --test simulation
cargo test --locked --offline --test simulation_contracts
cargo test --locked --offline --test simulation_crash
cargo run --locked --offline --example simulate -- 17 /tmp/brook-sim-17 /tmp/brook-sim-17.json
cargo run --locked --offline --example simulate -- /tmp/brook-sim-17.json /tmp/brook-replay-17 /tmp/brook-replay-17.json
cmp /tmp/brook-sim-17.json /tmp/brook-replay-17.json
```

Use fresh store directories. Both the example and host driver refuse an existing Brook database. The replay command reads the full recorded action list; it does not regenerate a schedule from a seed. Use the same source revision when replaying. Scenario format version 2 fixes the host commands, scripted response rules and fault semantics. Unknown versions and plans over 256 actions are rejected. Invalid work/session references produce a failure trace instead of indexing another session.

The example rewrites its JSON trace after each action and on a reported action/oracle failure. If observation fails, it preserves the original action error and the last good observation, and records the observation failure separately; the reused snapshot is not claimed to describe the failed action. The trace contains format version, seed, full scenario commands, effective graph configurations, quotas, immutable submissions and correlation IDs, authority clock epochs and elapsed logical times, actual contexts and harness names/results, durable histories/statuses/processor-delivery attempt counts, external effect observations, assertions and the failing action index. Nested reopen observations include before/after snapshots. Reply text appears in the observed outcome events; scripted response construction is fixed by the scenario version. This is a reproducible host-command transcript, not a log of every internal SQL statement or storage engine operation.

A failed action may leave a real transaction's committed result in the temporary store. The driver neither repairs it with SQL nor changes the expectation to match it. Investigate the saved failure and replay into another fresh directory. Process-kill tests use their own durable correlation fixture because an abruptly killed process cannot promise to finish a trace write.

All inputs are fixed generic fixtures. No credentials, network clients, model calls, personal context or production databases enter these runs. The trace deliberately includes synthetic payloads and contexts for assertions; it is not a redacted production telemetry format.

## What the composed workflow does

The seeded scenario creates two namespaces with identical external client/conversation identifiers. Each has three admitted requests. It executes a real configured filter → routing operator → uppercase transform → work node path, using `execute_processor` and pinned code identities. The router still selects the same logical branch after the transform is inserted.

The **host test driver** then creates the session causal message, explicit fake-sink grant and awaited Submission using the transformed input. It uses a stable operation ID derived from the delivery ID and keeps the correlation fixture in its own test data. This is host orchestration across separate commits. The production graph does not issue a tool request, own a continuation, or automatically route a completed job back into itself.

Requests A1 and A2 share a session; A2 replies before A1. Other-session work interleaves according to a fixed seeded generator. Newer session activity arrives before resumes; after A2 completes, another activity event and a restart precede A1. The driver delays, duplicates and conflicts admissions/replies, supplies a forged responder, cancels work, revokes another request's grant before send admission, expires ownership, and closes/reopens the real authority. A scripted Harness reads the actual current Context. A1's first physical result is discarded by a restart; a second invocation uses the existing EchoHarness, and only its logical result commits. A failing harness is settled through `fail_job`.

A pure test-only `ResultProcessor` accepts a correlated result that the host read from committed history. It performs no database, tool or external I/O. The host runs it through `execute_processor` and commits its proposal, after which the actual terminal effect adapter renders output. This demonstrates a recoverable test-host composition, **not** a durable two-way production handoff. A production bridge still needs explicit admission/cancellation/quota/correlation contracts and atomicity or a separately specified recovery protocol. The current `Harness` returns text/failure and cannot request tools; nonempty tool permissions remain unsupported.

## External effects versus durable knowledge

Fake tool effects and terminal output use separate files outside Brook's database. Tool journal entries record the actual effect ID, destination (sink/account/recipient) and payload exposed by SendAttempt. The oracle compares all five expected entries against independently specified fixture policy and requires the revoked sixth effect to be absent. Tests read those journals independently of Store status. A journal record means the fake outside world observed something; `sent`, `printed` or `unknown` describes Brook's knowledge. They are not interchangeable.

One fake tool effect is journaled and then its send remains unresolved across reopen. A correlated reply and successful continuation must not erase that uncertainty. One terminal writer writes and syncs its journal, then deliberately reports a flush error. Brook must retain `unknown`; retrying the same attempt cannot append another byte. The existing processor tests additionally inject a finalization SQL error after successful output. The new seeded driver performs no SQL writes, injected triggers or hidden repairs.

Physical harness invocation can repeat after interruption. The invariant is one committed logical result and one committed graph fanout, not one physical invocation. The process tests retain a separate physical-invocation journal to demonstrate the difference.

## Independent oracles and adversarial checks

The oracle uses expected fixture identities, semantic outputs, graph path cardinality and immutable event prefixes. It does not reimplement the runtime's SQL transitions. After each host action it checks append-only history, request/session ownership, at most one logical result, agreement with independently constructed expected text, complete configured child closure (existing row, parent, expected node, pinned graph, scope, unique ownership and no observed orphans), required output rows, complete bounded pending sets, monotonic processor-delivery attempt counts and no repeated physical terminal effect. It checks every actual harness Context, including EchoHarness invocations, against the admitted request identity, full agent configuration, instruction, payload, causal references and correlated reply, as well as cross-session evidence. Reopen observations additionally require unchanged durable history, unchanged external tool journals and preserved processor-delivery attempt budgets. There is no public logical-job attempt-count inspection in this slice; job attempt exhaustion is tested through claim behavior across restarts, and the external physical-invocation journal is a distinct observation.

Negative controls deliberately corrupt *copied observations*, never the running Store: duplicate logical result, missing or misbound child/parent/output rows, wrong graph/node/scope, orphan children, wrong effect IDs/destination accounts/recipients/payloads, reset processor-delivery attempt count, changed immutable Context fields, cross-session Context, duplicate external effect, forgotten unknown outcome and history rewind. Each must be rejected by the expected oracle check. These demonstrate that the oracle detects those observations; they are not mutation coverage of arbitrary Rust bugs.

Additional public-API contract scenarios cover:

- Cancellation before reply, after reply, after manifest admission and during execution; late work is rejected and a later same-session request can run.
- Activity between context build/admission and between manifest admission/claim; the host rebuilds from current evidence.
- An oversized UTF-8 harness result, explicit bounded failure settlement, repeated restarts through attempt exhaustion, and stale failure fencing.
- Invalid routes and output schemas, fanout all-or-nothing, state revision conflicts, quota exhaustion with reserved failure capacity, and a v1 queued delivery surviving v2 registration.
- Reversed branch selection with overlapping destinations: unique destination fanout is unchanged.
- Terminal authority revoked before versus after admission, plus escaped OSC/control text and normal Unicode/newline/tab output.

Metamorphic checks insert extra matching retries without changing final observations, reverse independent-session execution while consistently renaming namespace labels and normalizing global IDs, reverse branch ordering, and register an unused graph version. No equivalence is claimed for arbitrary same-session reordering or physical invocation counts across crashes. More schedule dimensions, automatic shrinking and broad arbitrary-graph generation remain future testing work.

## Real process kills

The new child-process suite kills the process without Rust destructors at six composed workflow boundaries:

1. Physical harness returned, before logical result commit.
2. Logical result committed, before host graph publication.
3. First child inserted inside the graph transaction, before commit.
4. Graph result/fanout committed.
5. Terminal write permission consumed, before actual output.
6. Terminal output written, before durable effect finalization.

Recovery uses public APIs and a persisted generic host-test correlation fixture. Tests require one logical result, one child fanout, stable receipt, the expected physical invocation count, and explicit uncertainty after either terminal interruption. They cannot infer whether an unknown terminal effect occurred from its database state alone: the external journal is empty in one case and contains the output in the other.

These are **six new kill cases**. The existing 18 core and 13 processor cases remain separate regression coverage, for 37 total cases. This is not a power-loss, torn-storage, failed-fsync or malicious-filesystem test.

## Evidence and limits

The default matrix runs seeds 0–31, varying other-session reply order and bounded delays while requiring A2 before A1. All schedules are finite and manually driven. The driver uses the actual advisory authority lock, SQLite transactions, generation fences and public state inspections; a pure mock state machine is not substituted for them.

This slice changes no production behavior, so no formal transition model is changed. Relevant existing ProcessorCommit (including the repeat-write negative control), LocalExecution, Ownership, Admission, SessionContext and Recovery configurations are rerun in an isolated copy. Their bounded checks assume storage atomicity and do not prove these tests or the Rust implementation correct.

There is no claim of liveness, distributed correctness, exactly-once physical invocation/display, untrusted extension isolation or exhaustive fault coverage. Retention collection, real provider behavior, native agent orchestration, feedback loops, arbitrary tool contracts, actual connector authorization and a durable graph/agent bridge remain outside this slice. The separate extensibility API documentation PR has not started.


### Validation of this slice

- Formatting and Clippy with warnings denied pass.
- The full Rust suite passes 69 tests: 18 new test functions and 51 existing tests. Three ignored child entrypoints are explicitly launched by their parent tests.
- The new matrix covers 32 seeds, each with 53 host commands. A seed-17 example and its full-action replay produce byte-identical JSON observations.
- Six new process-kill cases pass; all 31 existing core/processor cases pass, for 37 total.
- Existing processor and two-session demos pass.
- Isolated TLA regressions pass: ProcessorCommit (40 distinct states), LocalExecution (114), Ownership (99,733), Admission (63,574), SessionContext (3,568,797), Recovery (4,185), plus the expected ProcessorCommit repeat-write counterexample. Safe cases end with an empty queue. Tool SHA-256: `b3e56ba18c65abd22e35755739963000f22e770279841d364699c31951ede70a`.

No production defect was reproduced by these scenarios, and no production source or formal transition model changed. Independent review of the testing slice remains necessary; passing these finite scenarios is not proof of correctness.

Independent review strengthened the test oracles and failure reporting in this revision. Scenario v2 records structured external effect journals and separate observation failures; v1 replay plans are rejected rather than interpreted with changed semantics. These changes address test gaps, not reproduced production bugs.
