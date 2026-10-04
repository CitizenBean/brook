# Sessions, asynchronous context and authorized delivery

This document proposes concrete contracts for the [architecture draft](architecture.md). It is a design for review, not implemented behavior.

**Requirements:** route a message to the correct session; retain the context needed by asynchronous work; allow newer activity and multiple pending requests; preserve newer activity on return; and deliver outgoing messages only to their authorized sinks and recipients. A user may explicitly request another channel, such as asking an agent in a chat conversation to email someone. Correct routing must support that request.

**Proposed mechanics:** the identities, records, portable receipt/event protocol, retention policy and authorization boundaries below. The [bounded TLA+ checks](../spec/README.md) test small abstractions of these mechanics. They do not establish a complete security or implementation proof.

## Resolve the session from trusted ingress

Use this external lookup key:

```text
(authenticated namespace, trusted adapter client, external conversation ID)
    -> stable internal session ID
```

The namespace is an authenticated account/workspace boundary. The client identifies the configured adapter/client instance within that boundary; it is not an arbitrary payload field or an agent definition. The external ID identifies a conversation according to that adapter's contract. The same external ID in two clients or two namespaces is not the same session.

The trusted adapter obtains the namespace and client from its authenticated connection/configuration, then validates the external conversation ID against that connection's authority. An external ID can originate in a provider event, but its presence is not proof that the sender is authorized to use it. A payload's claimed namespace, client, internal session or output route cannot override the resolved destination. Unknown, ambiguous or unauthorized mappings fail explicitly. Automatic creation, when enabled, must create the mapping within the authorized namespace using a race-safe uniqueness constraint.

Treat the internal ID as an opaque stable identifier. Do not reuse it for a different conversation. Deliberate aliases or session merging require a separate authorized operation and are not part of this proposal or model. A client reconnect should resolve to the existing mapping rather than creating a new session accidentally.

The session determines history/context ownership and the acting principal. It does **not** grant unlimited access to every sink, recipient or conversation associated with that principal.

## Separate the source session, work destination and return

An agent-to-agent request has three distinct references:

| Reference | Meaning |
| --- | --- |
| Source session | Owns the request, original causal context and eventual continuation event |
| Destination | Agent/service plus an explicitly selected, authorized destination session or isolated work scope |
| Reply binding | Durable request/continuation identity and expected responder; resolves the return to the source session |

A request to another agent does not make that agent's session the reply's source conversation. Nor does an agent name or client ID identify a session by itself. Cross-namespace work requires an explicit capability; default routing cannot infer that permission.

Before dispatch, persist an immutable request record with at least:

- Request, logical effect and continuation IDs; originating logical call/event ID.
- Source internal session and namespace; source principal/authorization reference.
- Destination work address and expected responder identity/capability.
- Correlation token, expected reply schema and expiry/closure state.
- Captured session cancellation generation and context revision, as separate values.
- Causal history/checkpoint references, their format/harness metadata and retention pin.
- Outgoing intent reference and bounded attempt metadata.

These are conceptual fields, not final Rust types. Retries use the same logical request/effect and bindings. Attempt IDs are not new authority or new work identities.

An incoming reply is an authenticated response to a known request. Resolve the owning source session from that durable request. Check the responder binding, correlation/schema, request status, expiry and current source-session cancellation generation. Do not trust a reply's claimed receiving session, namespace or client; do not fall back to whichever session happens to be running. Request IDs are correlation references, not bearer authorization by themselves.

## Portable asynchronous execution

The proposed portable default avoids leaving an open provider-specific tool call indefinitely:

1. The harness calls Brook's asynchronous dispatch tool. Brook admits dependencies, persists the request, checkpoint references and outbox intent, and returns a **pending receipt** containing the request ID. This completes that provider tool invocation; the receipt is its immediate result.
2. The session's run may finish or continue. A waiting request does not hold the active-run slot. New user messages and other work can update the same session, and several requests can remain pending.
3. A valid later reply atomically claims the pending request, appends a correlated continuation outcome to the source session's durable history, and records a resume intent. The outcome refers to the originating logical call and request; it is not spliced into an old provider transcript as that call's delayed provider result.
4. The scheduler claims a logical resume, checks cancellation again, and builds a fresh bounded context using current session history/projection plus retained request-specific causal material. It can start a new run through any harness that supports this portable contract.

A harness may advertise a stronger suspended-tool or exact-execution resume capability. Use it only with an explicit supported protocol that preserves that harness's transcript rules and session concurrency guarantees. Portable reconstruction does not promise serialization of arbitrary runtime internals or continuity of the same execution.

One active run per session with a mailbox remains the proposed initial scheduling policy. History can advance while work is waiting. Acceptance order need not equal issue order: request B can complete and resume before request A. Every outcome remains attached to its own request. A reply for A does not restore the session to the revision at which A was issued.

Request-specific cancellation/timeout closes that request without changing the whole session's generation. A session reset/cancellation increments only that session's generation and invalidates its older pending continuations. Ordinary messages, context compaction and revision changes do not increment cancellation generation. Check generation at both reply acceptance and logical resume admission. Cancellation after an admitted run or external send cannot undo what already happened.

## Durable history and bounded context

Keep three related records distinct:

| Record | Role |
| --- | --- |
| Session history | Append-only, session-owned event records with stable event identity and revision/order metadata; immutable while retained |
| Current projection | Disposable bounded summary plus recent/relevant tail, recording the history range/references it covers |
| Request checkpoint | Portable retained references to the originating instruction, logical call/receipt, relevant causal material and reconstruction metadata |

A history revision advances only for its session's new durable activity. Compaction changes the view, not old event identities or cancellation generation. A summary is lossy; its coverage metadata is not a substitute for exact material a continuation requires.

At request creation, select and pin the material necessary for later interpretation of that request. Pinning must cover both pending replies and accepted outcomes awaiting materialization. A pinned record may be moved to another durable tier, but its referenced contents must remain recoverable. A retention collector must not discard it merely because the current projection no longer displays the original call. After terminal completion/cancellation, release the continuation's pin according to a defined retention policy; ordinary history retention is still independent.

At resume:

1. Load the accepted outcome and immutable request record from the source session.
2. Resolve the pinned causal references and validate their identity/version. Rebuild a missing projection from available durable history; never invent missing source material from a summary or model memory.
3. Read the current source-session revision. Select current relevant history and construct a request-scoped causal block containing the original instruction/call reference, pending receipt, and correlated outcome. Keep newer session activity intact.
4. Build a bounded prompt using that projection and causal block. Record included/covered references and the revision used. If the required block cannot fit, retrieve, compact safely, branch explicitly or fail according to a declared policy; silently truncating necessary causal material is not a valid fallback.
5. Admit the logical resume with a version/cancellation check. If the source history advanced during construction, rebuild or apply a deliberate merge/branch policy. Do not overwrite live history or the current projection with an old snapshot.

If retained material is missing, corrupt, expired contrary to the pin contract or unsupported by the chosen adapter, record an explicit `recovery_failed` outcome with the request/reason. Do not mark the logical continuation as successfully resumed. Retry, user escalation and manual recovery policy remain open. No eventual recovery is promised by the model.

In portable mode the originating call and eventual outcome form a **logical causal association**. The original provider tool-call/result pair is the dispatch call plus its pending receipt. The later continuation event is formatted as fresh input for the new run. Provider-specific pairing, role ordering and schema validation belong to the harness adapter; matching request IDs alone does not establish provider transcript validity.

## Authorize each outgoing message, then preserve its destination

A session has a default reply route, but an outgoing message has its own authorized destination. For example, a user in Discord may request “email this to my colleague.” Resolve which sending account and recipient are authorized, using the user instruction and policy; ask for missing or ambiguous recipient authority when necessary. That email intent can coexist with a separate Discord acknowledgement from the same session. Do not require every effect to return to the originating channel.

**Proposed outgoing intent binding:**

```text
logical effect ID + source session/principal + authorization/grant reference
+ resolved sink/provider + sending account + recipient/thread
+ relevant content/data scope + binding version/generation + expiry
```

The exact fields depend on the sink. A display name, raw route ID or model-generated email address is not an authorization grant. Policy decides whether the user instruction authorizes the specific recipient, account, action and data. An agent-generated suggestion and an authenticated user's instruction are different evidence.

Once authorized, persist that resolved binding with the outbox intent. Operators, replies, resumed runs and retries may carry the intent but cannot silently change its destination, account, scope or authority. Sending to a different destination requires a new separately authorized intent. A retry preserves the existing effect ID and bound destination.

Before admitting delivery, the trusted sink dispatcher verifies that the intent's authorization is still valid and that its binding/version matches the authorized record. Missing, revoked, expired, mismatched or recycled bindings fail closed; there is no fallback to another recipient, account or channel. Do not resolve an old route ID afresh to a newly assigned recipient. Binding generations must not have an ABA reuse within their identity lifetime.

A preflight check is insufficient if revocation or route reassignment can occur before admission. The model treats validation plus **delivery admission** as one linearization point. A real dispatcher must implement an equivalent fenced/transactional contract with its authorization state. This is not a claim that a database check and irreversible provider send can be globally atomic. After admission, an in-flight provider request may still complete even if permission is revoked. Stronger immediate revocation requires provider-side fencing/cooperation and is not promised here.

The delivery safety claim is: every admitted effect uses the destination authorized for that specific intent, with authority valid at admission. Later revocation does not retroactively invalidate that history. This does not promise eventual delivery, exactly-once provider effects, correctness of provider recipient resolution, or confidentiality of message content merely because the destination is correct. Trusted adapters, authorization policy and provider/account mappings remain part of the security boundary.

## Open implementation decisions

- Namespace/principal representation, authentication adapters, mapping creation, migration and deliberate aliases.
- Concrete capability/grant evaluation, recipient resolution, sink account ownership, expiry and revocation propagation.
- Storage transactions, ownership/fencing, active-run scheduling and crash recovery across history/outbox/resume records.
- Causal selection, pin size/expiry, retention quotas, summary quality, context budgets and missing-material escalation.
- Shared versus isolated agent destination sessions; permitted cross-namespace work.
- Harness capability negotiation and provider-specific receipt/continuation formatting.

The models check bounded examples of the proposed invariants and expose several unsafe alternatives. They assume trusted metadata and atomic abstract actions; they neither implement these boundaries nor prove that the independent models compose into a secure deployed system.
