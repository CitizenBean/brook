# Easy setup, connector management and the live DAG

**Accepted direction; mostly planned.** Brook should handle the common 80% of use cases with sensible defaults and a short guided setup. Users should not need to learn a stream-processing framework or author a large YAML graph to try an instance, connect a source and see results. The percentage is a product goal, not a measured coverage claim.

The current [processor slice](processors.md) provides a default terminal recipe, a small Rust builder, immutable validated graph registration, effective-configuration inspection and durable delivery inspection. It does **not** implement the connector control plane, a web UI/server, Home Assistant integration, external discovery, credential setup, an agent loop or real-time telemetry.

## One configuration system, several interfaces

Built-in recipes and minimal settings should cover ordinary pipelines. A small builder and optional advanced graph configuration should express overrides without creating a second execution model. All paths compile to the same validated graph, with the effective configuration available for inspection. Adding a transform between a router's logical branch and its destination should not require changing router code. There is no mandatory return to a dispatcher.

**Proposed API shape:** a typed management service supports discovery, configuration drafts, validation, preview, activation and status. The CLI, setup wizard, future web editor and an assisting agent call the same service. An agent should ask for the few missing choices and propose changes through these operations, not edit configuration files or infer authority from event text. Management is a control-plane responsibility; processors consume admitted events in the data plane. A route decision cannot activate a connector or change its credentials.

Useful typed operations would distinguish `ConnectorDiscovery`, `ConfigDraft`, `ValidationReport`, `ActivationPreview`, `ActiveConfigVersion` and `ConnectorStatus`. These are proposed domain types, not current Rust APIs. Draft revision checks and activation preconditions should reject stale previews. The service should show the concrete effect of activation: which sources will be read, which destinations may receive output, which scopes/state will be used, and which work keeps its existing binding. Graph registration/validation is the implemented foundation; connector lifecycle, permissions and transactional activation across components still need design and tests.

## Connect an instance with useful defaults

For a future Home Assistant connector, the intended happy path is: provide an instance URL and authorized access, inspect what the instance exposes, choose a useful recipe, preview it, and activate a versioned configuration. A setup agent can assist with interpretation and defaults. It must not convert discovery results into permission to make external changes.

Discovery is read-only by default and must use the access the user actually granted. Setup should select a bounded set of relevant entities/events, explain its choices, and let users narrow or override them. Endpoint validation, credential storage, access checks, local-network policy and reconnect behavior belong in the connector/control-plane design. Credentials should be opaque references to a dedicated store; they must not appear in event data, configuration previews, logs or telemetry.

Activation must distinguish local configuration changes from changes to the external system. External Home Assistant changes require explicit approval of a concrete preview. Discovery or adding a local subscription must not silently create devices, change automations or actuate entities. Revocation, invalid access and partial setup should produce an understandable status and a recoverable draft rather than partially active hidden configuration.

Active configurations are immutable versions. In-flight work pins graph, code, config, schema and destination-binding identities. Publishing a new configuration must not reinterpret an old delivery against current routing or silently change its destination. State compatibility and schema migration must be checked before activation; cutover, rollback, draining and connector replacement remain open contracts. The current runtime can register explicit graph versions; it has no mutable active-version pointer or connector activation transaction yet.

## A web view of structure and processing

The requested web view should display the configured DAG and overlay current processing: queue counts, active attempts, processing latency, errors, retry counts and selected routes. A user should be able to follow one event through its causal path, from ingress receipt to processor decisions and sink outcome, and see the persisted reason for each route or suppression. Configured edges and actual traversed paths must be visually distinct. Unknown external outcomes should remain visibly uncertain.

Stable store/graph/node/delivery/parent identities support this view; an internal numeric delivery ID alone is only unique within its store. The current implementation persists graph/node identity, graph version, delivery/parent/child IDs, attempt counts, status and routing reason. It does not yet persist timing metrics, provide queue aggregates, stream updates or serve a browser. A latency contract needs explicit timestamps and restart-safe clock semantics; it cannot be inferred from the current logical lease clock.

Default inspection should be read-only and redact payloads, state and secret-bearing metadata. The existing local inspection API omits payload and state, but scope keys/reasons/failures may still be sensitive and need access control and redaction before remote display. Secret values must never enter telemetry. Node labels, reasons and error text are data, not HTML or management instructions.

Graph edits, activation, replay and cancellation are separate authorized management actions. Opening a DAG or viewing a failed event grants none of those permissions. Replaying uncertain external effects needs adapter-specific evidence and a separately authorized intent; a “retry” button must not blindly resend. Cancellation cannot retract an already admitted effect.

## Follow-up constraints

A live view must not grow memory or persistent history without bound. Telemetry needs explicit buffer/retention quotas, backpressure or dropped-update markers, and a clear distinction between authoritative durable records and disposable presentation updates. After disconnection, a viewer should obtain a consistent snapshot and resume from an identified cursor, with visible gaps when incremental updates were dropped. Slow clients must not block event completion or consume its reserved failure capacity.

The next control-plane slice should define typed connector capabilities, draft/preview/activation semantics, credential references, authorization checks and consistent read snapshots before implementing a provider or web UI. Test stale activation, scope isolation, old in-flight bindings, partial failures, redaction, bounded telemetry, dropped updates and reconnect snapshots. Real connector setup and the optional agent loop remain separate work.
