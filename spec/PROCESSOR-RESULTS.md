# Processor commit model results

The focused `ProcessorCommit` model abstracts one admitted input delivery, one scoped state snapshot, two ownership generations, one concurrent state revision, two child intents, and a pending/printing/write_started/unknown terminal effect with a physical write counter. It explores valid and invalid route/authority snapshots and capacity of one or two children. This is a deliberately small safety abstraction, not execution of the Rust implementation.

Checked with TLC 2026.10.03.231403 (rev 1813307), Java 25.0.2, one worker, 1 GiB heap, seed 1 and fingerprint polynomial 0. Tool SHA-256: `b3e56ba18c65abd22e35755739963000f22e770279841d364699c31951ede70a`. The official release artifact is the same locally retained jar used for the earlier model checks; its exact hash identifies it.

```sh
python3 spec/check.py /tmp/tla2tools.jar --case ProcessorCommit
```

The safe configuration completed with 49 generated states, 40 distinct states and an empty queue. All 12 negative controls produced their named invariant counterexample. Mutation state spaces are stopped at the counterexample and are not exhaustively checked.

| Configuration | Generated | Distinct | Result / expected violated invariant |
| --- | ---: | ---: | --- |
| `ProcessorCommit-blindResend` | 49 | 40 | NoBlindResend |
| `ProcessorCommit-duplicate` | 3 | 3 | DeduplicatedAdmission |
| `ProcessorCommit-omitResult` | 27 | 27 | AtomicResult |
| `ProcessorCommit-omitState` | 27 | 27 | AtomicState |
| `ProcessorCommit-overQuota` | 25 | 25 | BoundedFanout |
| `ProcessorCommit-partialFanout` | 27 | 27 | AtomicFanout |
| `ProcessorCommit-repeatCommit` | 44 | 36 | CompletionOnce |
| `ProcessorCommit-repeatWrite` | 49 | 40 | SingleWrite |
| `ProcessorCommit-staleOwner` | 44 | 36 | FencedOwner |
| `ProcessorCommit-staleState` | 45 | 36 | FreshState |
| `ProcessorCommit-unauthorized` | 19 | 19 | AuthorizedRoutes |
| `ProcessorCommit-unknownRoute` | 23 | 23 | KnownRoutes |
| `ProcessorCommit` | 49 | 40 | safe; queue 0 |

The model checks admission deduplication, single logical completion, atomic state/result/fanout, ownership generation fencing, fresh state revision, explicit destination authority, valid route selection, capacity, and no blind resend after terminal uncertainty. The write-permission transition is separate from physical output. Failed finalization leaves that permission consumed; `repeatWrite` deliberately repeats physical output from that state and violates `SingleWrite`. A crash can happen after consumption before output, or after output before finalization. Atomic durable storage, trusted host schema/scope validation and immutable configuration are assumed at this level. Graph cycle validation, arbitrary processor code, cross-scope isolation, full migration behavior, large graphs, clocks, disk failures and refinement to SQL are covered only by separate reasoning/tests or remain limitations. There is no liveness or composition proof.

Relevant existing safe models were rerun in an isolated copy so historical checked-in logs and manifests remain untouched:

| Model | Distinct states | Queue |
| --- | ---: | ---: |
| LocalExecution | 114 | 0 |
| Admission | 63,574 | 0 |
| Ownership | 99,733 | 0 |
| Delivery | 331,776 | 0 |
| Routing | 124 | 0 |
| SessionContext | 3,568,797 | 0 |
| Retention | 1,394,936 | 0 |

The runner contains 113 configurations across 11 models. This slice ran the 13 new configurations and seven relevant existing safe configurations; it did not rerun every historical negative control. New source/log hashes are in `results/PROCESSOR-SOURCES.sha256`. Older manifests pin the runner version from their original checks.
