# Local execution crash boundaries

This addition accompanies the experimental local Rust core. It does not change the original nine models or 91 configuration sources/logs. The suite now has ten models and 100 configurations. The original [results](RESULTS.md) remain historical evidence for their recorded revision.

`LocalExecution.tla` starts with one durably admitted request. It composes a send-attempt marker, an authenticated reply and durable resume intent, one logical job, up to two physical execution attempts, two crashes and three authority incarnations. An unresolved admitted send becomes unknown on restart. An unfinished physical execution returns to the same durable job, while old completion tokens remain available as adversarial inputs. Pins protect both the logical job and unresolved delivery/recovery: early job completion cannot release them while a send remains in flight or unknown. Confirmed send success reevaluates release. The `terminalUnpin` mutation reproduces the premature-release race found during independent implementation review.

The safe model completed with **142 generated states, 114 distinct states and zero states left on queue**. All eight mutations stopped at the expected invariant violation:

| Configuration | Expected invariant | Distinct states |
| --- | --- | ---: |
| `LocalExecution-terminalUnpin` | `LiveDependenciesPinned` | 38 |
| `LocalExecution-blindResend` | `NoBlindResend` | 18 |
| `LocalExecution-volatileJob` | `DurableJob` | 26 |
| `LocalExecution-splitReply` | `AtomicReply` | 8 |
| `LocalExecution-earlyDone` | `DoneHasResult` | 15 |
| `LocalExecution-earlyUnpin` | `LiveJobPinned` | 15 |
| `LocalExecution-repeatCommit` | `CompletionOnce` | 52 |
| `LocalExecution-staleAttempt` | `FencedAttempt` | 79 |

Logs are in `results/LocalExecution*.log`. The tested-source hashes are in `results/LOCAL-EXECUTION-SOURCES.sha256`; `SOURCES.sha256` remains unchanged to preserve provenance of the earlier suite and its earlier runner.

Reproduction uses the same official tool binary as the original results:

- TLC `2026.10.03.231403`, revision `1813307`.
- JAR SHA-256 `b3e56ba18c65abd22e35755739963000f22e770279841d364699c31951ede70a`.
- OpenJDK 25.0.2; one worker, 1 GiB heap, parallel GC, seed 1, fingerprint polynomial 0.

```sh
python3 spec/check.py /tmp/tla2tools.jar --case LocalExecution
# Repeat with each named mutation, or run the full suite without --case.
```

Atomic durable transitions, authenticated replies, correct policy decisions, lock exclusivity and recoverable pinned contents remain assumptions. The model omits cancellation, context revision races, physical storage failure, resource accounting, provider reconciliation and real harness execution. It contains no fairness or liveness claim. In particular, bounded attempt exhaustion can leave work inspectable without eventual execution. This focused state machine is not a proof that the Rust transactions refine it or that all ten models compose.

For this implementation review, Admission, Ownership, Recovery and Delivery were also independently rerun from the unchanged architecture baseline; all safe queues exhausted with their previously recorded counts. The other original configurations were inspected through their saved results and source hashes, not rerun.
