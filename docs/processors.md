# Local processors and terminal runtime

This experimental implementation stacks on published local-core commit `67fd430be680f6284649c9bb3b286e46c45431e4`. It implements a bounded durable DAG of native Rust processors and a terminal producer/sink. It does not implement an agent loop, providers, Wasm, timers, arbitrary awaited dependencies, background scheduling or feedback cycles. The separate compaction experiment remains parked at `28f6ff07f02ee09a895fcfe9ce70502172d15eaf` and is not included.

## Start without a configuration file

```sh
cargo run --locked -- processor-demo /tmp/brook-processors-new
cargo run --locked -- terminal /tmp/brook-terminal sample-1 'hello'
cargo run --locked -- terminal-uppercase /tmp/brook-terminal sample-2 'hello'
cargo run --locked -- graph /tmp/brook-terminal 2
cargo run --locked -- inspect /tmp/brook-terminal 1
cargo run --locked -- terminal-run /tmp/brook-terminal
```

The demo requires a fresh directory. Ordinary terminal commands reopen the same store. Supply a stable operation ID: repeating the same ID, graph version and input recovers the original receipt; changing the input or graph under that ID conflicts. The default recipe is input → blank filter → router's `display` branch → terminal. Its local namespace, terminal client and default conversation are verified host constants, never payload fields. `terminal-uppercase` selects recipe version 2 with an uppercase transform between router and output. The router code, its configuration and logical branch remain unchanged.

Defaults are 1,024 permanently retained deliveries, 16 graph versions, 64 destination bindings, eight child deliveries per commit, 4 KiB serialized event/configuration input, 4 KiB serialized state, 16 KiB serialized proposal and three evaluation attempts. The manual runner releases its scope lease on all post-claim error returns, including recovered deliveries whose attempt budget is exhausted. It processes at most 64 pending deliveries per invocation; call `terminal-run` again if work remains. This is a local trusted CLI, not a remote authentication service. An `inspect` or `graph` command opens the single authority and performs normal restart recovery; it cannot attach to an already-running authority.

For a custom text path, the builder avoids envelope, schema and edge boilerplate:

```rust
let graph = PipelineBuilder::terminal(&identity, binding)
    .prepend("normalize", Version::new("uppercase", 1), serde_json::Value::Null)?
    .prepend("filter", Version::new("filter", 1), serde_json::Value::Null)?
    .build();
store.register_graph(&graph)?;
```

`terminal_recipe` and the builder produce the ordinary `Graph` type. `validate_graph`, `register_graph` and `effective_graph` use the same validation and immutable representation for recipes, builder output and advanced raw graphs. There is no separate trusted shortcut or YAML language. The builder currently defaults to text events and nullable counter state; raw nodes can select the supported primitive/object schemas, initial state, routing branches and version identities. More recipes and ergonomic typed schema derivation are later work.

## Processor and Operator contracts

A **Processor** is a general executable graph component. An **Operator** is a Processor with the routing capability enabled in its pinned node configuration. It can transform and route in one evaluation. Ordinary transforms return `ConfiguredNext`; routing operators may return `SelectedBranches`. Graph configuration connects those logical branches to concrete next nodes. Paths can chain transforms, other operators and sinks without returning to a dispatcher. Routing does not create authorization.

`Processor::process` receives an immutable `ProcessingAttempt` with a versioned event, same-scope state snapshot, pinned node configuration and budgets. `TypedProcessor` plus `RustAdapter<P>` supplies concrete Rust input/state/output types; adapters decode only the values the host supplied. The host checks the adapter's code ID/version before invoking it through `execute_processor`. Native extensions and host callers are trusted. Version labels are deployment identities, not verified binary hashes, sandboxing, CPU limits or proof of deterministic execution. Native code must avoid external effects during evaluation; the host cannot prevent a malicious extension from bypassing it.

A proposal contains replacement state, zero or more output values, a route choice and a bounded routing reason. All outputs use the same selected destination set: fanout is the Cartesian product of outputs and unique selected destinations. Independently routed heterogeneous outputs are not supported by this proposal type. Zero output requires explicit `Suppress(reason)`; accidental empty output does not silently complete work. Suppression can still commit a state update. Output and state schemas are checked independently. This slice validates versioned JSON shapes (text, object, integer, boolean, nullable counter), not a full JSON Schema language. Domain adapters can perform richer validation. Compaction, retrieval and harness APIs remain separate typed domain roles; future graph adapters can call them without making their contracts a universal decision object.

The default state scope is `(namespace, pipeline, node, key)`. Terminal keys derive from the verified client and conversation, with unambiguous bounded identifiers. Workers cannot choose another scope, edit the input envelope or supply arbitrary state keys. Two independent keys can execute independently. Graph versions share a node/key's state intentionally: schema changes require a separately designed migration or a new node/pipeline identity. An incompatible state schema fails atomically even when the new graph itself is structurally valid.

## Durable records and ownership

Processing uses separate graph, binding, state, ingress and delivery tables. It does not force fanout into session events or the original one-outbox-per-request schema. Events need not be conversations; the terminal is simply the first producer. The current ingress adapter maps a terminal conversation to a state key, while the event envelope itself is versioned schema plus data.

Admission resolves an immutable graph version and atomically records the input delivery, initial scoped state, and ingress deduplication record before returning a receipt. Deduplication is scoped by verified namespace/client/conversation and operation ID. The v1 canonical intent explicitly encodes its semantic version, pipeline, graph version and payload; generated envelope fields are not retry-comparison inputs. IDs are stable within the store. Parent IDs and committed child IDs preserve causation. IDs and deduplication records are never recycled in this slice.

A processing lease binds store identity, authority incarnation, scope, worker and persistent generation. Claim is allowed only when the scope is unowned or its heartbeat lease has expired. Heartbeat and protected writes recheck the live generation and deadline in the transaction. Restart invalidates every lease; takeover returns interrupted evaluations to pending, but moves interrupted terminal effects to `unknown`. Ownership changes preserve work identity and state. The existing local authority lock, SQLite FULL-sync transactions and logical-time limitations apply; see [local core](local-core.md).

`prepare_processing` durably admits a bounded physical evaluation attempt and snapshots state revision, event, graph, code/config/schema versions and binding identity. It does not hold a database transaction while extension code executes. Concurrent proposals against the same state revision may exist. A commit with a stale state revision returns an explicit conflict and requeues the same delivery only while its attempt budget remains; the caller must prepare and evaluate again. Exhaustion persists failure, without creating a new delivery or renewing retry allowance.

`commit_processing` validates ownership, physical attempt, state revision, schemas, scope, logical routes, sink authority, serialized byte budgets and child capacity. One immediate transaction updates scoped state, records the decision/result, inserts **every** child delivery, and completes the input. Failure rolls back all of those changes. Duplicate completion recovers the recorded outcome without another state mutation or fanout; the outcome remains authoritative even if the repeated proposal differs. Invalid proposals remain available for explicit `fail_processing`; the bundled runner settles them as bounded failures. No universal fallback route is inferred. Production critical-input failure policy remains future configuration work.

All downstream deliveries retain the original graph version. Graph registration rejects replacement of an existing version; new registration cannot rewrite queued work. Node snapshots pin code identity, config version and contents, input/output/state schema identities and definitions, and terminal binding ID/version. Binding versions are immutable and can be revoked, never rebound or silently reactivated. The host checks the authorized verified identity when creating a terminal child and again at terminal effect admission. Choosing a different branch cannot grant access to a different client/conversation. A configured route to an unauthorized destination causes the entire fanout transaction to fail.

## Terminal effects and uncertainty

The terminal is both a producer and a sink. The host supplies a verified `TerminalClient` capability and associates the actual writer with that connection. The current CLI uses the local process's stdout; applications using another writer are responsible for verifying its connection mapping. Payload claims cannot select another terminal identity.

`begin_terminal` commits `printing` before exposing a print attempt. `print_terminal` atomically consumes that permission by changing `printing` to `write_started` under the live lease before any I/O. Only that successful transition permits writing and flushing, followed by `printed` or `unknown`. A cloned attempt cannot write again even if finalization fails with a transient SQLite error while its lease remains valid. The host can retry only finalization when it retains evidence of the original result; it must not repeat the write. A partial write or flush failure is uncertain. A process kill before writing, during writing, or after output but before the final commit also becomes `unknown` on recovery. `printed` means the writer reported successful write/flush and that result was recorded; it does not prove a human saw the text.

A crash after the write-permission commit but before actual output also becomes unknown; the host cannot distinguish it from a completed external write. Unfinalized `write_started` is never eligible for another write. Unknown attempts cannot return to pending, and the same completed attempt cannot print again. There is no exactly-once display claim or blind retry. Reconciliation and an explicitly authorized replay protocol are deferred. Revocation after effect admission cannot retract an admitted effect. Passing a stale token after takeover cannot commit its result; the effect may still have happened externally.

The terminal sink escapes ANSI/OSC introducers, C0/C1 controls, carriage returns, DEL and Unicode bidirectional direction controls into visible Unicode escape text. Ordinary Unicode, newline and tab remain readable. There is no raw-output opt-in in this slice. Stored payloads are unchanged; rendering is an effect-adapter concern. This prevents event text from issuing cursor, screen or clipboard instructions to the terminal.

## Bounds, migration and inspection

Every delivery permanently consumes one quota slot and a conservative logical reservation for its input, bounded state/result, child references and failure metadata. Completion and failure use their admitted row's reserved allowance. Child fanout must reserve all child slots in its commit. Quota exhaustion does not prevent recording failure for already-admitted work. Graphs are capped at 64 nodes and 256 KiB; each node's configured edges and each selected output fanout are bounded. Inspection scans accept at most 64 deliveries. Logical byte accounting includes JSON/UTF-8 encoding and metadata, but is not a physical SQLite/RSS bound. Native extension allocation, runtime, panic behavior, disk-full and power-loss behavior remain outside this validation.

Graph registration rejects all cycles, including dormant edges. Retries retain the same delivery identity and do not add graph edges. This conservative DAG restriction implements the approved current slice; future feedback scope and dynamic awaited-dependency handling remain open. Seven-day collection remains unimplemented. All live and terminal records are retained under hard quotas, so there is no premature deletion of in-flight evidence or deduplication identity.

Database schema 1 upgrades transactionally to schema 2 by adding processing tables. Existing session requests, history, grants, pins and stored job contexts remain unchanged. Original `Submission` retries now compare decoded semantic values rather than raw JSON formatting. No new Submission fields are introduced; future defaulted fields must keep a version-aware semantic decoder and migration tests rather than compare fresh serialization to old bytes. Limits mismatch rolls back schema changes; unknown database or processing schema versions fail before mutation. The paused compaction branch also used an experimental schema 2: its databases are rejected here, and integrating that branch requires a deliberate combined migration.

`inspect_processing` returns graph version, namespace/pipeline/node/key, delivery/parent/child IDs, status, attempts, failure and routing reason, without event payload or state. `committed_processing` is a separate trusted API exposing the full outcome. Graph configuration inspection is distinct from runtime inspection. Reasons and failure strings are extension/host-supplied text and must not contain secrets; they still need policy-based redaction before any remote viewer. A future [control plane and live DAG view](control-plane.md) can build on these identities. No web server, live subscription stream or remote management authorization is implemented.

## Evidence and remaining review

Rust tests exercise recipe overrides, arbitrary typed adapters, routing transforms, direct chains, one/many branches, invalid routes/schemas/scope, whole-fanout authorization rollback, deduplication, state conflicts, lease fencing, pinning, independent keys, quotas, migrations and terminal uncertainty. Thirteen processor child-process kill cases cover before/after ingress, evaluation, a partially inserted fanout inside its transaction, before/after processor commit, before/after terminal admission and write-permission consumption, written output and before/after effect completion. The original 18 crash cases remain passing.

The [ProcessorCommit model](../spec/PROCESSOR-RESULTS.md) checks a small atomic-commit abstraction with explicit negative controls. It assumes SQLite transaction atomicity, does not execute this Rust code, and is not a refinement proof, liveness proof, or proof of combined models. Independent review is still required before publication.
