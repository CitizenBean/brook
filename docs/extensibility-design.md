# A small extension API over the durable core

**Proposal, not an implemented or stable API.** Keep durable admission,
ownership fencing, proposal validation and atomic commit in the host. Give
extension authors a typed interface that does not expose `Store`, leases or
envelope bookkeeping. The [current guide](extensibility.md) documents what
compiles now; [research notes](extensibility-research.md) explain the sources.

Acceptance covers a tested current-API example and a reviewed design direction;
it does not validate the proposed facade's usability or compatibility.

## Authoring and registration

A processor consumes typed input, configuration and a state snapshot, then
returns proposed state, outputs and a routing decision. An Operator is a
routing-specialized Processor. It chooses declared logical branches, never
credentials, arbitrary sink addresses or permissions.

Proposed convenience helpers cover `map`, `filter`, `stateful` and `route`.
Stateless authors should not invent counters or write lease/error cleanup.
The host wraps helper results in the same bounded proposal protocol used by
full processors. Start with one input/output schema per node and one routing
decision per proposal; defer heterogeneous output ports.

This is a **nonworking author-experience sketch**, not Rust code to copy:

```text
registry.register(map("uppercase", |text: String| text.to_uppercase()))
registry.register(stateful::<Counter>(config))
draft.input("source").then("classify")
draft.branch("classify", "display").then("uppercase").to("terminal")
validated = compiler.validate(draft, registry, authorized_bindings)
management.preview(validated)
```

The full typed trait should associate `Input`, `Output`, `State` and `Config`.
A registration factory decodes config, validates semantic constraints, and
constructs a configured evaluator before activation. Registration must not
contact external systems or produce effects. Dynamic discovery is a separate
read-only management operation.

Each registry entry binds code identity to config schema, input/output/state
schema identities, initial-state construction, routing capability and
execution class. Reject duplicate identities. Store the canonical config and
its identity in the graph; retain registered implementations needed by pinned
work. Missing code must produce an explicit blocked status, never silently
select the newest implementation. Registration is a trusted-host declaration,
not a signature or a sandbox.

Use generic adapters for typed authoring and an erased, dyn-compatible host
interface for a heterogeneous registry. Do not expose the erased JSON layer
as the default tutorial. Keep `ProcessingAttempt` on the host side: authors
receive an owned evaluation input and return an owned proposal. Rust lifetimes
must not allow a callback to retain a Store borrow across evaluation.

### Contract under review

These proposed Rust signatures are **nonworking sketches**, with supporting
types omitted. They are not exported by Brook or compiled by the consumer tests.

```rust,ignore (proposed contract; supporting types are not implemented)
trait Evaluator {
    type Input;
    type State;
    type Output;
    type Config;
    type Error: std::error::Error + 'static;

    fn configure(config: Self::Config) -> Result<Self, Self::Error>
    where Self: Sized;

    fn evaluate(
        &self,
        input: Self::Input,
        state: Self::State,
        context: EvaluationContext,
    ) -> Result<Decision<Self::State, Self::Output>, Self::Error>;
}

impl Registry {
    fn register<P: Evaluator + 'static>(
        &mut self,
        descriptor: Descriptor<P>,
    ) -> Result<RegistrationId, RegistrationError>;
}

impl GraphCompiler {
    fn validate(
        &self,
        draft: GraphDraft,
        registry: &Registry,
        bindings: &AuthorizedBindings,
    ) -> Result<ValidatedGraph, ValidationReport>;
}
```

Recommend `&self` with host-serialized calls per configured instance. The
factory consumes typed config and stores it in that instance; config is not
passed on every evaluation. Input, state and context are owned values. Context
contains bounded metadata, not Store access or forgeable authority tokens.

The descriptor supplies the adapter that maps the associated extension error
to a bounded `ProcessorFailure` at the host boundary. It preserves a stable
category and safe diagnostics; the host alone assigns retry disposition.
Registration returns a code-factory handle after descriptor checks succeed.
`RegistrationError::DuplicateIdentity` rejects an existing code identity without
replacement, even for identical descriptors. Graph validation invokes that
factory for each node's typed config and reports config/construction failures
in `ValidationReport`. Thus one registered code identity supports multiple
separately configured instances without sharing mutable configuration.

Recommend collecting independent validation diagnostics up to a fixed limit,
in stable node/path order, with an explicit truncation marker. Skip dependent
checks whose prerequisites failed. This gives useful feedback without an
unbounded collect-all pass; invalid input never yields a `ValidatedGraph`.

## Compile once, activate deliberately

Introduce distinct `GraphDraft` and opaque `ValidatedGraph`. Recipes, the Rust
builder, configuration files and a future web editor use the same compiler.
Validate code availability, config semantics, initial state, schema compatibility,
branch existence, graph shape, quotas and permitted binding scope together.
Return a bounded `ValidationReport` with node/path, stable error code and useful
message. An unchecked graph cannot be activated through the new facade.

A forward-reading builder and explicit branch-path insertion should preserve
router identity when adding a transform. Reject duplicate nodes/branches at
the edit that introduces them. Keep low-level drafts available for generated
graphs; avoid making map surgery the documented happy path.

Validation is not permanent authorization. Activation rechecks draft revision,
registry generation, state compatibility and current grants in the committing
transaction. Delivery and effect admission recheck applicable authorization.
Publishing a new version cannot redirect existing work or reuse incompatible
state. State is currently shared by namespace/pipeline/node/key across graph
versions; changing the state schema requires an explicit migration or new scope.
Migration, rollback and draining need a separate reviewed contract.

## Execution, failure and identity

Start with synchronous, bounded native evaluation. Treat async evaluation and
durable external work as different future interfaces. A future async facade
needs an explicit boxed-future erasure choice, backpressure, cancellation and
shutdown contract; `async fn` alone does not provide heterogeneous dispatch.

For the first registry, propose host-serialized calls per configured instance,
without adding `Send + Sync` to every author trait. A later worker-pool adapter
may require `Send`; shared concurrent instances must explicitly require `Sync`
and define safe caches. Durable state lives in snapshots and committed proposals,
not callback fields. No extension may rely on a callback running only once.

Invocation cancellation fences commits belonging to that invocation. Pipeline
draining stops new ingress admission while allowing already admitted work to
settle; its completion policy remains to be specified. Neither operation undoes
already admitted effects or forcibly stops arbitrary native code. Timeouts need
cooperative checks or an isolated execution process; a blocking-thread timeout
does not terminate that thread. Bound queue depth, evaluations in flight,
input/output/state bytes, retries and diagnostic size. Retention defaults to
seven unused days when implemented, with live references protected; extension
metadata cannot release those references.

Replace `retry: bool` with host-owned `FailureDisposition` and structured
`ProcessorFailure` categories: invalid input/config, transient evaluation,
budget exceeded and extension fault. Extensions report facts; host policy
decides bounded retries or dead-lettering. Effect outcomes remain a separate
type: applied, confirmed not applied, or unknown. Unknown requires evidence
reconciliation or a newly authorized intent, never generic retry policy.

Use newtypes for identities likely to be confused, such as code, schema and
binding IDs. Separate wire envelope version, Rust API version, code version,
config schema/version, state schema/version and graph version. They have
different compatibility rules. Plan non-exhaustive public diagnostics and
private validated fields before stability; do not promise a Rust dynamic-library
ABI. WIT is a later portable boundary with its own versioned imports/exports.

## Separate extension roles

| Role | Extension returns | Host retains |
|---|---|---|
| Processor / Operator | State/output/route proposal | Admission, scope, leases, commit, authorized destinations |
| Producer | External record plus stable source identity | Atomic deduplication/admission; admission ack only after durable acceptance |
| Sink | Effect evidence or unknown outcome | Per-intent grants, effect admission, recovery policy |
| Harness | Bounded result from portable context | Session isolation, fresh continuation admission, tool/effect authority |
| Compactor | Versioned historical representation | Source coverage/digest, required raw references, commit fences |
| Retriever | Candidate references and ranking | Authorized-scope provenance checks, raw evidence and context budgets |
| Management adapter | Discovery facts or configuration draft | Preview, explicit authority, revision-checked activation |

Shared descriptors should provide identity and diagnostics without forcing all
roles into one trait. Two harnesses must consume the same portable continuation
manifest; neither can restore an old transcript over newer session context.
Compaction/retrieval names here describe a future integration of the separate
checkpoint, not available APIs. Lossy history never becomes instructions or
authorization; missing required evidence fails closed.

A producer admission ack confirms local durable acceptance, not successful
downstream processing or delivery. Any end-to-end protocol ack needs its own
completion and failure contract. For retrieval, session-history references must
belong to the same session. External-corpus RAG is allowed through separately
authorized scopes with source identity/provenance checks and context budgets;
it does not inherit session or tool authority.

## Decisions and costs

| Current problem | Recommendation | Tradeoff |
|---|---|---|
| Raw config fails during execution | Typed factory + registration validation | More descriptor machinery; still need runtime bounds |
| Simple transform needs state/envelope ceremony | Helpers over one proposal protocol | Helper defaults must remain inspectable |
| Hardcoded demo dispatch | Explicit heterogeneous registry | Version retention and missing-code policy required |
| Unvalidated builder and reverse prepend | Draft/compiler/validated graph + forward paths | Two graph representations; low-level escape hatch needs care |
| Retry flag mixes policy and failure | Structured failure and host disposition | More types, clearer recovery decisions |
| Native/async/effect contracts blurred | Distinct execution classes | More adapters; avoids false cancellation/delivery promises |

## Delivery sequence and acceptance tests

1. **Authoring facade and registry.** Add typed config, descriptors and helpers
   without changing transaction semantics. External consumer tests: a trivial
   map has no counter/lease ceremony; typed state survives restart; invalid
   config, duplicate code and descriptor mismatch fail before activation.
2. **Graph compiler and composition.** Route through a named branch, insert a
   transform, and prove router code/config unchanged. Reject empty/duplicate
   branches and incompatible schemas. Exercise recipe/builder/import parity,
   stale activation, revoked bindings and old-version work retention.
3. **Failure and execution contract.** Test conflict reevaluation, bounded
   retries, cancellation before/after evaluation, fenced late commits,
   diagnostic limits and queue saturation. Keep existing crash boundaries and
   uncertainty tests. Add a compile test for the chosen erased interface.
4. **Separate bridges.** Before implementing connectors, prove source ack after
   durable admission, duplicate ingress, partial activation and discovery without
   mutation. Before a harness bridge, run two harnesses over identical portable
   inputs and resume after newer context without rewinding it. Before compaction
   integration, test cross-session references, missing required evidence and
   live-reference retention. Terminal admission must remain distinct from
   displayed/unknown status throughout.

Before stabilizing the facade, add external-consumer compile-pass and
compile-fail gates for each public boundary: callers cannot construct host
capabilities, mutate validated graphs or interchange identity newtypes; the
chosen erased evaluator remains dyn-compatible. Pair negative cases with valid
construction/use cases so unrelated compiler failures cannot masquerade as
protection. [Rustdoc `compile_fail` tests](https://doc.rust-lang.org/rustdoc/write-documentation/documentation-tests.html#compile_fail)
can express rejected examples; check the intended diagnostics and test the
supported Rust versions. Keep serialized fixtures for wire/config/state schema
evolution separate from Rust source/semver compatibility tests. These are future
acceptance gates, not coverage supplied by the current consumer example.

This proposal does not implement those steps. The bounded TLA+ models and
simulation configurations remain useful regression evidence, not a proof of
implementation correctness or a substitute for the acceptance tests above.
