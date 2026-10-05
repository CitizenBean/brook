# Verified TLC results

The suite now contains **91 configurations**: nine safe models, 60 mutation checks, 21 positive reachability witnesses and one explicitly ephemeral-profile boundary witness. All have their expected results. This revision executed all 47 new admission/ownership/recovery/retention configurations and reran the existing safe `Continuations` and `Delivery` regressions. The other 42 published results are preserved byte-for-byte; the five earlier models and all 44 earlier configurations are unchanged from the published baseline.

## Local reliability revision

- Source baseline: `d384568b6de14fc115ee7c293d3fedf058f23ff8`.
- Tool: `TLC2 Version 2026.10.03.231403 (rev: 1813307)`, official `v1.8.0/tla2tools.jar` release URL.
- Tool SHA-256: `b3e56ba18c65abd22e35755739963000f22e770279841d364699c31951ede70a`.
- Runtime/options are unchanged: OpenJDK 25.0.2, `-XX:+UseParallelGC -Xmx1g`, one worker, seed 1, fingerprint polynomial 0, breadth-first exploration.
- Reproduce all cases with `python3 spec/check.py /tmp/tla2tools.jar`, or select one with `--case Admission` (likewise `Ownership`, `Recovery`, `Retention` or a configuration name).
- The new modules/configurations/runner and retained sources are identified by [SOURCES.sha256](results/SOURCES.sha256). No symmetry, state constraints or depth truncation are used; no fairness or liveness is asserted. Negative checks stop at their expected first counterexample.

| New safe model | Bounds | Generated | Distinct | Queue at completion |
| --- | --- | ---: | ---: | ---: |
| Admission | 2 scoped operation keys, 2 payloads, 2 token incarnations; durable profile | 260767 | 63574 | 0 |
| Ownership | 2 sessions, 2 workers; time 0–4, duration 2, at most 2 grants/session | 390173 | 99733 | 0 |
| Recovery | 2 effects/sessions, 2 attempts/effect, shared notification budget 4 | 12022 | 4185 | 0 |
| Retention | 3 operations, disk 7/RAM 1 abstract units, unused TTL 2, retry/work intervals 3, time 0–6 | 6642347 | 1394936 | 0 |

The retention ticks are independent bounded test intervals, not literal production days. The agreed configurable unused-record default is seven days; the model does not select a production pending-work deadline. It checks capacity and collection safety, not eventual cleanup.

## New checks

| Configuration / trace | Generated | Distinct | Queue at stop | Result |
| --- | ---: | ---: | ---: | --- |
| [Retention](results/Retention.log) | 6642347 | 1394936 | 0 | Completed; all configured invariants pass |
| [Retention-overDisk](results/Retention-overDisk.log) | 3755 | 2123 | 1281 | Expected mutation failure: `BoundedDisk` |
| [Retention-overRam](results/Retention-overRam.log) | 15 | 11 | 7 | Expected mutation failure: `BoundedRam` |
| [Retention-collectPinned](results/Retention-collectPinned.log) | 94 | 78 | 50 | Expected mutation failure: `LivePinsRecoverable` |
| [Retention-collectSnapshot](results/Retention-collectSnapshot.log) | 17 | 17 | 11 | Expected mutation failure: `CurrentSnapshotRetained` |
| [Retention-dropExpired](results/Retention-dropExpired.log) | 253 | 198 | 122 | Expected mutation failure: `TerminalBeforeReclaim` |
| [Retention-forgetWindow](results/Retention-forgetWindow.log) | 64 | 46 | 29 | Expected mutation failure: `RetryWindowEnforced` |
| [Retention-duplicateAfterGC](results/Retention-duplicateAfterGC.log) | 8023 | 4329 | 2828 | Expected mutation failure: `NoReadmission` |
| [Retention-controlWitness](results/Retention-controlWitness.log) | 105 | 85 | 53 | Reachability witness: `NoControlProgress` |
| [Retention-backpressureWitness](results/Retention-backpressureWitness.log) | 11 | 11 | 7 | Reachability witness: `NoBackpressure` |
| [Retention-unpinWitness](results/Retention-unpinWitness.log) | 261 | 203 | 125 | Reachability witness: `NoCleanupAfterUnpin` |
| [Retention-staleWitness](results/Retention-staleWitness.log) | 4972 | 2866 | 1743 | Reachability witness: `NoStaleRetryRejection` |
| [Recovery](results/Recovery.log) | 12022 | 4185 | 0 | Completed; all configured invariants pass |
| [Recovery-wrongSession](results/Recovery-wrongSession.log) | 35 | 27 | 14 | Expected mutation failure: `RecoveryEvidence` |
| [Recovery-unknownAsFailed](results/Recovery-unknownAsFailed.log) | 42 | 32 | 17 | Expected mutation failure: `RecoveryEvidence` |
| [Recovery-duplicateNotice](results/Recovery-duplicateNotice.log) | 111 | 52 | 25 | Expected mutation failure: `AtMostOneRecoveryEvent` |
| [Recovery-renewBudget](results/Recovery-renewBudget.log) | 624 | 278 | 114 | Expected mutation failure: `BoundedRecoveryNotifications` |
| [Recovery-blindRedrive](results/Recovery-blindRedrive.log) | 31 | 24 | 11 | Expected mutation failure: `AuthorizedSafeRedrive` |
| [Recovery-freshIdentity](results/Recovery-freshIdentity.log) | 180 | 94 | 42 | Expected mutation failure: `StableEffectIdentity` |
| [Recovery-unknownWitness](results/Recovery-unknownWitness.log) | 42 | 32 | 17 | Reachability witness: `NoUnknownNotification` |
| [Recovery-investigationWitness](results/Recovery-investigationWitness.log) | 443 | 203 | 87 | Reachability witness: `NoInvestigatedEmailRedrive` |
| [Ownership](results/Ownership.log) | 390173 | 99733 | 0 | Completed; all configured invariants pass |
| [Ownership-stealLive](results/Ownership-stealLive.log) | 29 | 13 | 9 | Expected mutation failure: `ClaimOnlyEligible` |
| [Ownership-racyClaim](results/Ownership-racyClaim.log) | 658 | 126 | 83 | Expected mutation failure: `ClaimOnlyEligible` |
| [Ownership-staleCommit](results/Ownership-staleCommit.log) | 753 | 241 | 175 | Expected mutation failure: `FencedWrites` |
| [Ownership-staleHeartbeat](results/Ownership-staleHeartbeat.log) | 634 | 241 | 175 | Expected mutation failure: `HeartbeatOnlyLive` |
| [Ownership-expiredRenew](results/Ownership-expiredRenew.log) | 477 | 187 | 137 | Expected mutation failure: `HeartbeatOnlyLive` |
| [Ownership-reuseGeneration](results/Ownership-reuseGeneration.log) | 569 | 242 | 176 | Expected mutation failure: `FencedWrites` |
| [Ownership-globalClaim](results/Ownership-globalClaim.log) | 3 | 3 | 1 | Expected mutation failure: `SessionLocalOwnership` |
| [Ownership-takeoverCancels](results/Ownership-takeoverCancels.log) | 3 | 3 | 1 | Expected mutation failure: `OwnershipPreservesWork` |
| [Ownership-transferWitness](results/Ownership-transferWitness.log) | 1609 | 633 | 447 | Reachability witness: `NoTransferredResume` |
| [Ownership-reacquireWitness](results/Ownership-reacquireWitness.log) | 129 | 65 | 49 | Reachability witness: `NoReacquisition` |
| [Ownership-heartbeatWitness](results/Ownership-heartbeatWitness.log) | 106 | 52 | 39 | Reachability witness: `NoHeartbeatExtension` |
| [Admission](results/Admission.log) | 260767 | 63574 | 0 | Completed; all configured invariants pass |
| [Admission-earlyReceipt](results/Admission-earlyReceipt.log) | 8 | 8 | 5 | Expected mutation failure: `ReceiptRecoverable` |
| [Admission-splitCommit](results/Admission-splitCommit.log) | 7 | 7 | 4 | Expected mutation failure: `CompleteAdmission` |
| [Admission-retryDuplicate](results/Admission-retryDuplicate.log) | 96 | 79 | 51 | Expected mutation failure: `OneLogicalAdmission` |
| [Admission-replaceMismatch](results/Admission-replaceMismatch.log) | 102 | 83 | 53 | Expected mutation failure: `OneLogicalAdmission` |
| [Admission-ignorePayload](results/Admission-ignorePayload.log) | 285 | 182 | 98 | Expected mutation failure: `ReceiptMatchesCaller` |
| [Admission-omitScope](results/Admission-omitScope.log) | 20 | 15 | 6 | Expected mutation failure: `ReceiptMatchesCaller` |
| [Admission-volatileCausal](results/Admission-volatileCausal.log) | 18 | 13 | 5 | Expected mutation failure: `CompleteAdmission` |
| [Admission-dropRecovery](results/Admission-dropRecovery.log) | 44 | 39 | 25 | Expected mutation failure: `RecoveryScansCommitted` |
| [Admission-receiptWitness](results/Admission-receiptWitness.log) | 754 | 478 | 266 | Reachability witness: `NoRecoveredReceipt` |
| [Admission-dispatchWitness](results/Admission-dispatchWitness.log) | 137 | 100 | 60 | Reachability witness: `NoRecoveredDispatch` |
| [Admission-mismatchWitness](results/Admission-mismatchWitness.log) | 102 | 83 | 53 | Reachability witness: `NoMismatchRejection` |
| [Admission-scopeWitness](results/Admission-scopeWitness.log) | 270 | 169 | 87 | Reachability witness: `NoScopedAdmissions` |
| [Admission-ephemeralWitness](results/Admission-ephemeralWitness.log) | 36 | 31 | 19 | Weaker-profile boundary witness: `ReceiptRecoverable` |

## New trace readings and limits

- **Admission:** commit, crash before the pending receipt is delivered, restart, resubmit the same scoped ID/payload, reuse and acknowledge the original receipt token. Recovery scanning also makes its original intent dispatchable. Counters and externally observed receipts survive crashes in the model; unsafe early receipts, partial commits, duplicate/replaced work and lost records cannot erase their evidence.
- **Ephemeral profile:** `Submit → Commit → Acknowledge → Crash` loses the in-memory records while the external receipt observation remains. This deliberately violates the durable assertion under `Durable = FALSE`; it is not a defect reported in the durable configuration.
- **Ownership:** worker 1 claims, authority time reaches expiry, worker 2 claims a fresh generation and resumes the existing pending continuation. Cancellation generation stays unchanged. Other witnesses show heartbeat extension and release/reacquisition by the same worker with a new generation. The racy-claim trace records competing preflights before both are incorrectly granted.
- **Recovery:** an uncertain email outcome is quarantined and notified to the originating session; a trusted authorization decision plus authoritative evidence of non-application permits redrive under the same effect identity. Notification failures spend the shared budget rather than creating fresh recursive budgets. No successful provider retry or resolution of ambiguity is claimed by moving to the DLQ.
- **Retention:** two body/control reservations and the current snapshot fill the seven-unit budget; a terminal transition still succeeds from its reserved control space. A record pinned past its unused TTL is completed/unpinned and then collected. Old IDs remain rejected after marker collection; forgetting the retry floor exposes an actual second admission in the dedicated negative control. Live snapshots and pending bodies remain protected.

The two repeated legacy safe checks retained their previous counts: `Continuations` 73,387 generated / 22,116 distinct; `Delivery` 1,843,201 generated / 331,776 distinct, both with empty queues. Their current logs are linked in the retained table below.

The new slices assume atomic local storage, trusted identifiers/evidence/policy, comparable authority time, persistent generations and accurate abstract accounting. They do not prove a storage engine, crash-safe clock policy, consensus, physical worker exclusivity, provider fencing, exact byte usage, semantic evidence quality or composition between models. The [local reliability document](../docs/local-reliability.md) separates accepted requirements from proposed mechanisms. The [model guide](README.md) describes durable/volatile/ghost state and each abstraction. Fingerprint collision estimates remain in the safe logs.

## Retained session/routing results

The following 44-configuration report describes the earlier published slice. Its numerical results and counterexamples remain applicable to the unchanged models; it is not a claim that all 44 were newly rerun for this local-reliability edit.


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
