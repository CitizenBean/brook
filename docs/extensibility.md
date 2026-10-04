# Write an extension with the current API

This guide describes Brook 0.1.0 at the published simulation baseline
`2b30f973df610133d0b953ab8a333e77cec718ab`. The API is experimental.
The [design proposal](extensibility-design.md) names future APIs separately.
These tests and the reviewed design direction do not validate the proposed
facade's usability or compatibility.

## Start with a typed processor

The external [consumer crate](../examples/extensibility-consumer/src/lib.rs)
implements a prefix transform with typed input, state and output. It uses only
public Brook APIs. Run its tests from the repository root:

```sh
cargo test --locked --manifest-path examples/extensibility-consumer/Cargo.toml
```

Implement `TypedProcessor`, then wrap it in `RustAdapter`. Today, configuration
is still `serde_json::Value`; the example's `PrefixConfig::parse` is application
code, not a Brook registration hook. It rejects unknown fields and oversized
prefixes. Its graph factory calls that validation before returning a graph;
the processor also checks config when invoked directly.

The following current API example is compiled as a consumer doctest:

```rust
use brook::processing::{Routing, TypedProcessor};
use brook_extensibility_consumer::Prefix;
use serde_json::json;

let result = Prefix.process(
    "message".into(),
    brook_extensibility_consumer::Count { processed: 0 },
    &json!({"prefix": "note: "}),
)?;
assert_eq!(result.outputs, ["note: message"]);
assert_eq!(result.state.processed, 1);
assert!(matches!(result.routing, Routing::ConfiguredNext));
# Ok::<(), brook::Error>(())
```

Processors return proposals. The host prepares an opaque `ProcessingAttempt`,
checks code identity during `execute_processor`, and uses `commit_processing`
to atomically update state, create children and complete the input. The
consumer test exercises that full public path and verifies a duplicate commit
does not create more children. A proposal alone is not a durable result.

Do not perform network writes or other irreversible work inside evaluation.
An evaluation can repeat after a crash, lease change or state conflict.
Native Rust code has the host process's OS authority; these traits provide no
sandbox, memory ceiling or forced timeout.

## Describe the graph accurately

`Node` carries code identity, config version, input/output/state schemas,
initial state, routing capability and edges. `RustAdapter` decodes Rust types,
but does not derive these descriptors. The example explicitly replaces the
builder's nullable-counter state with an object schema and matching initial
state. `JsonShape::Object` checks only the outer JSON kind, not its fields.

`PipelineBuilder` currently starts at a text terminal and prepends text nodes
with nullable-counter state. Prepend order is reverse execution order.
`build()` returns an unvalidated `Graph`; call `validate_graph` and
`register_graph`. Registration repeats structural checks and checks terminal
binding namespace, but does not prove an implementation exists or validate
its configuration. There is no active-version pointer.

`route(node, branch)` moves that node's `next` edges into one logical branch.
Calling it twice leaves the second branch empty and fails graph validation.
The consumer regression test records this limitation. For today's recipe,
`insert_uppercase` demonstrates inserting a transform after a logical router
without changing router code/config. It edits the public graph map and is
specific to that recipe. General path insertion is proposed work.

Each output currently goes to every selected unique destination; outputs
cannot individually select different routes. A routing operator is a
`Processor` whose node has routing enabled, not a separate execution engine.

## Keep host responsibilities outside extension logic

The consumer's small heterogeneous map demonstrates `Box<dyn Processor>` with
two successfully invoked implementations, each against its matching node.
Its host orchestration is a test fixture with success-path cleanup, not a
production runner: early errors abort the test and drop the temporary store.
Production hosts must settle failed attempts and release ownership on every
exit path. Brook has no registry yet: the terminal demo dispatches
hardcoded example names. A claimed code version is not binary attestation.
The current traits have no `Send`/`Sync` requirement, cancellation protocol or
async method. Hosts must choose scheduling and concurrency explicitly.

Use the host-verified `TerminalClient` and pinned binding when admitting
terminal work. `submit_terminal` returns a durable local admission ID, not
proof of display. `begin_terminal` admits an effect; `print_terminal` records
write start before touching the writer. Restart or ambiguous I/O can leave an
unknown outcome. Do not turn unknown into an automatic resend. See the full
[processor contract](processors.md) for revocation and fencing details.

The existing `Harness` trait accepts portable `Context` plus an output budget.
`EchoHarness` and `SummaryHarness` are two synchronous examples. They do not
implement provider authentication, a tool loop or durable suspension. The
awaited-request core and processor graph still lack a production bridge.
Compaction/retrieval work is a separate checkpoint, absent from this baseline.

The current suite verifies durability and recovery examples; it does not prove
an arbitrary extension correct. Seven-day unused retention with live-reference
protection, connector lifecycle and resource isolation remain unfinished.
