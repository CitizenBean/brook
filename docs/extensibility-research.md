# API research behind the proposal

Reviewed 2026-10-04 against the current Brook source and the primary sources
below. Recommendations are design judgments for Brook, not guarantees supplied
by these projects. Versioned links pin the documentation version observed;
unversioned references are living documents, not dependency selections.

| Source and observed contract | Consequence for Brook | Limit of the comparison |
|---|---|---|
| [Rust API Guidelines: flexibility](https://rust-lang.github.io/api-guidelines/flexibility.html) discuss generics and trait objects as distinct API choices. | Keep typed authoring generic; erase types at registry dispatch. | Generics alone do not produce a heterogeneous registry. |
| [Rust API Guidelines: type safety](https://rust-lang.github.io/api-guidelines/type-safety.html) recommend meaningful types and builders for complex construction. | Give identity domains and failure dispositions distinct types; simplify graph construction. | Newtypes cannot establish authorization or persistence. |
| [Rust API Guidelines: future proofing](https://rust-lang.github.io/api-guidelines/future-proofing.html) discuss control over implementation and representation. [Cargo compatibility guidance](https://doc.rust-lang.org/cargo/reference/semver.html) identifies source-breaking API changes. | Keep validated fields private; decide extension-trait and diagnostic evolution before stability. | Sealing author traits would prevent the third-party implementations Brook wants; do not seal those by default. |
| [Rust Reference: dyn compatibility](https://doc.rust-lang.org/reference/items/traits.html#dyn-compatibility) restricts dispatchable methods, including opaque async returns. | Test the erased interface from an external crate; choose future erasure deliberately before an async registry. | `Send`, `Sync`, cancellation and ABI compatibility are separate decisions. |
| [Tower 0.5.3 `Service`](https://docs.rs/tower/0.5.3/tower/trait.Service.html) separates readiness from calls returning futures. | Make capacity and admission visible in any async execution adapter. | Tower readiness is not durable admission, effect deduplication or commit fencing. Do not replace Brook's transactional core with a service call. |
| [Tokio 1.53.2 `spawn_blocking`](https://docs.rs/tokio/1.53.2/tokio/task/fn.spawn_blocking.html) explains that started blocking work cannot be aborted and that CPU work needs explicit concurrency limits. | Specify bounded concurrency and honest timeout behavior; use isolation when forced termination is necessary. | Tokio is not currently a Brook dependency; merely using a blocking pool would not sandbox extensions. |
| [Benthos public service v4.81.0](https://pkg.go.dev/github.com/redpanda-data/benthos/v4@v4.81.0/public/service#RegisterProcessor) registers a processor name, configuration specification and constructor. | Bind descriptors, validation and implementation construction in one registry entry. | This is an authoring precedent, not evidence of Brook's durability or permission semantics. |
| [Apache Beam user-code requirements](https://beam.apache.org/documentation/programming-guide/#requirements-for-writing-user-code-for-beam-transforms) warn that functions may be retried and external effects require care. | Document repeatable evaluation separately from one committed local result. | Brook's local transaction, scopes and effect uncertainty are its own contracts. |
| [OpenTelemetry custom components](https://opentelemetry.io/docs/collector/extend/custom-component/) exposes separate component-building paths, including receivers, connectors and extensions. | Keep producer, processor, sink and management responsibilities distinct. | Telemetry component categories are not a ready-made Brook plugin interface. |
| [Pi extension guide](https://raw.githubusercontent.com/badlogic/pi-mono/main/packages/coding-agent/docs/extensions.md) provides explicit tool/command/provider registration and describes extensions running with process permissions. | Offer focused registration surfaces and state native trust honestly. | Pi lifecycle events and session machinery must not become the portable Brook harness contract. This mutable reference was checked, not pinned to a release. |
| [WIT reference](https://component-model.bytecodealliance.org/design/wit.html) defines component interfaces and worlds. [Wasmtime component API](https://docs.wasmtime.dev/api/wasmtime/component/index.html) generates Rust bindings from WIT and links host imports. | Later derive a small portable evaluation boundary with explicit capabilities and versioning. | No WIT interface, Wasm runtime, fuel policy or stable plugin ABI is implemented here. |

## Alternatives rejected for the first slice

A single universal plugin trait would mix data transformation, external I/O,
session execution and privileged configuration. Separate roles make each
authority boundary reviewable while sharing identity metadata.

Making every extension asynchronous now would expand scheduling, cancellation
and dyn-dispatch decisions before fixing configuration and composition. Keep
the first facade synchronous; preserve a separate path for durable external
work instead of letting retriable transforms send directly.

A fully generic graph could catch some schema errors at Rust compile time,
but recipes, imported configuration and a future editor still need runtime
validation. Use one runtime compiler, with typed builders as an ergonomic
front end. Do not maintain separate rules for each interface.

Automatically retrying all failures is incompatible with uncertain sink
outcomes. Preserve typed effect evidence and keep retry policy under the host.
The [delivery sequence](extensibility-design.md#delivery-sequence-and-acceptance-tests)
turns these recommendations into specific implementation gates.
