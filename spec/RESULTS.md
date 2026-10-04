# Verified TLC results

All **44 configurations** produced their expected results: five safe models completed exhaustive breadth-first exploration with empty queues, 31 mutations produced the specified counterexamples, and eight deliberately false assertions produced positive reachability witnesses. The original checks were rerun alongside the new models. After strengthening the multi-session ordering witness, the final `SessionContext` safe configuration and all of its mutation/witness configurations were rerun.

## Reproduction and tested tool

- Repository baseline for this revision: `95718d201228680539cb8289fa0bf53fca77415d`.
- Official download: `https://github.com/tlaplus/tlaplus/releases/download/v1.8.0/tla2tools.jar`.
- Actual banner: `TLC2 Version 2026.10.03.231403 (rev: 1813307)`. This is the downloaded artifact’s banner despite the release URL label.
- Tool SHA-256: `b3e56ba18c65abd22e35755739963000f22e770279841d364699c31951ede70a`.
- Runtime: Homebrew OpenJDK 25.0.2, macOS aarch64, `-XX:+UseParallelGC -Xmx1g`.
- Command: `python3 spec/check.py /tmp/tla2tools.jar`.
- TLC options: `-workers 1 -seed 1 -fp 0 -noGenerateSpecTE -difftrace`; no depth/state constraints, symmetry reduction or simulation.
- All configurations disable deadlock checks. None asserts fairness or liveness.
- [SOURCES.sha256](results/SOURCES.sha256) records the exact tested `.tla`, `.cfg` and runner bytes. See [README.md](README.md) for runner validation, limitations and commands.

## Bounds

| Model | Finite domain |
| --- | --- |
| Waits | Three work nodes, attempts 0–2 per ordered pair |
| Continuations | Two requests, one session, revisions 0–2, cancellation generations 0–1, attempts 0–2 |
| Routing | Two namespaces × two clients × one shared external conversation ID, four internal sessions, two source/destination request records |
| SessionContext | Two sessions; two requests in A and one in B; one fresh message per session; generation 0–1; six possible A-history items and four B-history items; full/compact/empty projections; recoverability boolean per session |
| Delivery | Two intents from one chat session, four endpoint identities across two namespaces; own chat and email authorized; at most two admission attempts; one version/incarnation change per endpoint |

`Waits` and `Continuations` numeric bounds are in their configurations. The new models name their small finite sets directly in the modules; their configurations select mutations/invariants. Safe cases disable mutations. Changing these bounds or the implementation requires fresh verification. The largest safe run (`SessionContext`) completed in 2 min 8 sec on this machine; the runner allows 180 seconds per case.

## Completed checks

| Configuration / trace | Generated | Distinct | Queue at stop | Result |
| --- | ---: | ---: | ---: | --- |
| [Delivery](results/Delivery.log) | 1843201 | 331776 | 0 | Completed; all configured invariants pass |
| [Delivery-unauthorized](results/Delivery-unauthorized.log) | 10 | 6 | 4 | Expected mutation failure: `AuthorizedIntent` |
| [Delivery-payloadTarget](results/Delivery-payloadTarget.log) | 400 | 239 | 214 | Expected mutation failure: `ImmutableDestination` |
| [Delivery-lateLookup](results/Delivery-lateLookup.log) | 3600 | 1493 | 1252 | Expected mutation failure: `ImmutableDestination` |
| [Delivery-staleCheck](results/Delivery-staleCheck.log) | 3580 | 1477 | 1237 | Expected mutation failure: `AuthorizedAtAdmission` |
| [Delivery-reuseVersion](results/Delivery-reuseVersion.log) | 3584 | 1492 | 1251 | Expected mutation failure: `AuthorizedAtAdmission` |
| [Delivery-emailWitness](results/Delivery-emailWitness.log) | 982 | 524 | 463 | Reachability witness: `NoCrossChannelDelivery` |
| [Delivery-twoDestinationsWitness](results/Delivery-twoDestinationsWitness.log) | 60260 | 16338 | 10936 | Reachability witness: `NoTwoDestinations` |
| [Routing](results/Routing.log) | 2977 | 124 | 0 | Completed; all configured invariants pass |
| [Routing-spoofWitness](results/Routing-spoofWitness.log) | 3 | 3 | 1 | Reachability witness: `NoIgnoredSpoof` |
| [Routing-payloadRoute](results/Routing-payloadRoute.log) | 3 | 3 | 1 | Expected mutation failure: `TrustedIngress` |
| [Routing-omitNamespace](results/Routing-omitNamespace.log) | 3 | 3 | 1 | Expected mutation failure: `TrustedIngress` |
| [Routing-omitClient](results/Routing-omitClient.log) | 4 | 4 | 2 | Expected mutation failure: `TrustedIngress` |
| [Routing-payloadReturn](results/Routing-payloadReturn.log) | 19 | 11 | 9 | Expected mutation failure: `ReturnToOrigin` |
| [Routing-destinationReturn](results/Routing-destinationReturn.log) | 18 | 10 | 8 | Expected mutation failure: `ReturnToOrigin` |
| [Routing-wrongPeer](results/Routing-wrongPeer.log) | 18 | 10 | 8 | Expected mutation failure: `AuthenticatedReply` |
| [SessionContext](results/SessionContext.log) | 23316142 | 3568797 | 0 | Completed; all configured invariants pass |
| [SessionContext-closeOthers](results/SessionContext-closeOthers.log) | 203 | 113 | 96 | Expected mutation failure: `RequestLocalClosure` |
| [SessionContext-otherCancelWitness](results/SessionContext-otherCancelWitness.log) | 2634 | 986 | 783 | Reachability witness: `NoOtherCancelResume` |
| [SessionContext-compactionWitness](results/SessionContext-compactionWitness.log) | 1311 | 536 | 431 | Reachability witness: `NoCompactedRecovery` |
| [SessionContext-wrongReturn](results/SessionContext-wrongReturn.log) | 19 | 16 | 13 | Expected mutation failure: `OwnedReturn` |
| [SessionContext-copyOther](results/SessionContext-copyOther.log) | 187 | 99 | 83 | Expected mutation failure: `ContextIsolation` |
| [SessionContext-rollback](results/SessionContext-rollback.log) | 1264 | 501 | 399 | Expected mutation failure: `PreserveCausalAndNewer` |
| [SessionContext-unpin](results/SessionContext-unpin.log) | 26 | 21 | 18 | Expected mutation failure: `RetainedWhilePending` |
| [SessionContext-fabricate](results/SessionContext-fabricate.log) | 1348 | 551 | 444 | Expected mutation failure: `NoFabricatedRecovery` |
| [SessionContext-globalCancel](results/SessionContext-globalCancel.log) | 9 | 8 | 6 | Expected mutation failure: `SessionLocalCancellation` |
| [SessionContext-globalRevision](results/SessionContext-globalRevision.log) | 4 | 4 | 2 | Expected mutation failure: `SessionLocalRevision` |
| [SessionContext-orderWitness](results/SessionContext-orderWitness.log) | 750788 | 169467 | 97693 | Reachability witness: `NoOutOfOrderCompletion` |
| [SessionContext-failureWitness](results/SessionContext-failureWitness.log) | 1341 | 551 | 444 | Reachability witness: `NoRecoveryFailure` |
| [Waits](results/Waits.log) | 1119745 | 212544 | 0 | Completed; all configured invariants pass |
| [Continuations](results/Continuations.log) | 73387 | 22116 | 0 | Completed; all configured invariants pass |
| [Waits-racy](results/Waits-racy.log) | 291 | 130 | 89 | Expected mutation failure: `NoWaitCycle` |
| [Waits-retry](results/Waits-retry.log) | 44 | 29 | 20 | Expected mutation failure: `RetryIdentity` |
| [Continuations-earlyDispatch](results/Continuations-earlyDispatch.log) | 5 | 5 | 3 | Expected mutation failure: `RecoveryBeforeDispatch` |
| [Continuations-retryIdentity](results/Continuations-retryIdentity.log) | 68 | 57 | 41 | Expected mutation failure: `StableEffectIdentity` |
| [Continuations-splitAccept](results/Continuations-splitAccept.log) | 70 | 59 | 43 | Expected mutation failure: `AtomicAcceptance` |
| [Continuations-duplicate](results/Continuations-duplicate.log) | 286 | 196 | 136 | Expected mutation failure: `AtMostOneClaim` |
| [Continuations-duplicateResume](results/Continuations-duplicateResume.log) | 2921 | 1522 | 944 | Expected mutation failure: `AtMostOneResume` |
| [Continuations-stale](results/Continuations-stale.log) | 271 | 184 | 129 | Expected mutation failure: `ValidAcceptance` |
| [Continuations-staleResume](results/Continuations-staleResume.log) | 969 | 567 | 372 | Expected mutation failure: `ValidResume` |
| [Continuations-overwrite](results/Continuations-overwrite.log) | 861 | 514 | 338 | Expected mutation failure: `PreserveNewerActivity` |
| [Continuations-correlation](results/Continuations-correlation.log) | 72 | 60 | 44 | Expected mutation failure: `ValidAcceptance` |
| [Continuations-missingCall](results/Continuations-missingCall.log) | 285 | 195 | 135 | Expected mutation failure: `ToolCallResultPairing` |
| [Continuations-advancedWitness](results/Continuations-advancedWitness.log) | 247 | 175 | 123 | Reachability witness: `NoAdvancedReplyAccepted` |

Negative rows stop at their first counterexample, so a nonempty queue is expected. Witness rows run unmutated behavior and intentionally refute a “never happens” assertion; they do not demonstrate inevitable progress.

## What the new traces demonstrate

- **Out-of-order multi-session work:** prepare `a1`, `a2` and `b1`; accept `a2` before `a1`; resume `a2` before `a1`; accept and resume `b1`. Both origin sessions are active when `a2` is accepted. All three logical continuations finish.
- **Session-local cancellation:** prepare and accept `b1`, cancel session A, then resume `b1` in B. The global-cancellation mutant fails `SessionLocalCancellation`.
- **Compaction recovery:** prepare and accept `a1`, compact its session so the original call is absent from visible context, then reconstruct and resume using retained references.
- **Explicit recovery failure:** prepare and accept `a1`, lose its archive, then transition to `recoveryFailed`. The fabricated-recovery mutant instead resumes without recoverable contents and is rejected.
- **Pinned retention:** normal reclamation cannot delete active continuation material; removing that guard exposes the violation immediately.
- **Cross-session isolation:** wrong-session return and other-session context reconstruction respectively violate `OwnedReturn` and `ContextIsolation`. Global revision changes and closure of unrelated requests also fail their local-scope invariants.
- **Trusted ingress:** the same external conversation ID belongs to four separate namespace/client keys. Dropping either scope or trusting a payload destination misroutes it; the positive spoof witness ignores the payload claim.
- **Authenticated return:** substituting a payload session, confusing the target work session with the origin, or allowing an unexpected responder produces a counterexample.
- **Authorized cross-channel output:** a chat-origin intent authorized for email is admitted to email even when the untrusted payload suggests chat. Another witness admits separate chat and email intents from the same source.
- **Retargeting and revocation:** `Authorize → Check → Rotate → Admit` exposes late route lookup and recycled versions; `Authorize → Check → Revoke → Admit` exposes stale authorization checks. A payload-selected endpoint can also differ from the immutable authorized target.

The original mutation traces remain included: racy wait admission, renamed retry identity, early dispatch, reply claim without durable resume, duplicate claims/resumes, stale acceptance/resume, context rollback, bad correlation and missing logical call/outcome association. The original advanced-context witness still accepts a valid delayed reply after revision advances.

## Interpretation

These are bounded checks of proposed abstract contracts. Atomic state transitions, authenticated metadata, resolved endpoints, policy predicates and retained-reference semantics are assumptions. No refinement/composition proof, provider transcript validation, actual storage recovery protocol or comprehensive security proof is supplied. In particular, delivery admission is not an atomic irreversible provider send, and in-flight revocation is not guaranteed.

Full traces and TLC fingerprint-collision estimates remain in the logs. The largest run reports an optimistic fingerprint-collision estimate on the order of 10^-6; bounded TLC exploration is not an unbounded mathematical proof. Local parsing paths and process IDs are sanitized. See [the design](../docs/context-and-routing.md) and [model limitations](README.md#assumptions-and-limits).
