# Verified TLC results

Both safe configurations completed breadth-first exploration with an empty queue. All 12 mutation checks produced their expected counterexamples. One additional negative assertion produced the intended advanced-context acceptance witness. The runner exited successfully.

- Source baseline: `24ff656d73626b8cf010022ca1216705ec4627a8`. No repository instructions (`AGENTS.md`) were present in that source archive.
- Official download: `https://github.com/tlaplus/tlaplus/releases/download/v1.8.0/tla2tools.jar`.
- Actual tool banner: `TLC2 Version 2026.10.03.231403 (rev: 1813307)`. This is the banner of the downloaded artifact, despite the release URL label.
- Tool SHA-256: `b3e56ba18c65abd22e35755739963000f22e770279841d364699c31951ede70a`.
- Runtime: Homebrew OpenJDK 25.0.2, macOS aarch64; `-XX:+UseParallelGC -Xmx1g`.
- Command: `python3 spec/check.py /tmp/tla2tools.jar`; TLC options: `-workers 1 -seed 1 -fp 0 -noGenerateSpecTE -difftrace`.
- Wait bounds: `Work = {a,b,c}`, `MaxAttempts = 2`; both unsafe switches false in the safe case.
- Continuation bounds: `Requests = {r1,r2}`, `MaxRevision = 2`, `MaxGeneration = 1`, `MaxAttempts = 2`; `Mutation = "none"` in the safe case.
- No depth/state constraints, symmetry reduction or simulation were used. Deadlock checks are disabled; no fairness or liveness properties are asserted.

| Configuration / trace | Generated | Distinct | Queue at stop | Result |
| --- | ---: | ---: | ---: | --- |
| [Waits](results/Waits.log) | 1119745 | 212544 | 0 | Completed; all invariants pass |
| [Continuations](results/Continuations.log) | 73387 | 22116 | 0 | Completed; all invariants pass |
| [Waits-racy](results/Waits-racy.log) | 291 | 130 | 89 | Counterexample: `NoWaitCycle` |
| [Waits-retry](results/Waits-retry.log) | 44 | 29 | 20 | Counterexample: `RetryIdentity` |
| [Continuations-earlyDispatch](results/Continuations-earlyDispatch.log) | 5 | 5 | 3 | Counterexample: `RecoveryBeforeDispatch` |
| [Continuations-retryIdentity](results/Continuations-retryIdentity.log) | 68 | 57 | 41 | Counterexample: `StableEffectIdentity` |
| [Continuations-splitAccept](results/Continuations-splitAccept.log) | 70 | 59 | 43 | Counterexample: `AtomicAcceptance` |
| [Continuations-duplicate](results/Continuations-duplicate.log) | 286 | 196 | 136 | Counterexample: `AtMostOneClaim` |
| [Continuations-duplicateResume](results/Continuations-duplicateResume.log) | 2921 | 1522 | 944 | Counterexample: `AtMostOneResume` |
| [Continuations-stale](results/Continuations-stale.log) | 271 | 184 | 129 | Counterexample: `ValidAcceptance` |
| [Continuations-staleResume](results/Continuations-staleResume.log) | 969 | 567 | 372 | Counterexample: `ValidResume` |
| [Continuations-overwrite](results/Continuations-overwrite.log) | 861 | 514 | 338 | Counterexample: `PreserveNewerActivity` |
| [Continuations-correlation](results/Continuations-correlation.log) | 72 | 60 | 44 | Counterexample: `ValidAcceptance` |
| [Continuations-missingCall](results/Continuations-missingCall.log) | 285 | 195 | 135 | Counterexample: `ToolCallResultPairing` |
| [Continuations-advancedWitness](results/Continuations-advancedWitness.log) | 247 | 175 | 123 | Counterexample: `NoAdvancedReplyAccepted` |

All safe-run invariant names are listed in [README.md](README.md) and the corresponding configuration files. Negative rows stop early, so their remaining queue is expected. The advanced-witness case uses the unmutated specification and violates only the deliberately false `NoAdvancedReplyAccepted` assertion.

## Counterexample readings

- **Concurrent admission:** check A→B and B→A against the empty graph, then commit both stale preflights. `NoWaitCycle` fails.
- **Retry identity:** admit an edge or dispatch a request, then rename its logical edge/effect on retry.
- **Early dispatch:** send before preparing any durable recovery records.
- **Split acceptance:** prepare, dispatch, record an accepted reply without a resume intent. A crash at this boundary leaves the durable protocol incomplete.
- **Duplicate claim/resume:** repeat acceptance; the claim counter reaches two. The separate resume check permits that earlier error to proceed and shows `Prepare → Dispatch → Accept → Resume → Accept → Resume`, with `runs[r1] = 2`.
- **Stale acceptance:** prepare at generation 0, cancel to generation 1, dispatch, then accept the old-generation reply when the current-generation fence is removed.
- **Stale resume:** prepare, dispatch, accept at generation 0, cancel to generation 1, then resume when the second generation fence is removed.
- **Overwrite:** prepare at revision 0, add activity item 1, accept and resume from only the old checkpoint. The new context is empty while durable activity contains item 1.
- **Correlation:** accept a reply whose correlation does not match the request being claimed.
- **Missing tool call:** materialize a result without its originating call.
- **Advanced-context witness:** prepare at revision 0, advance to revision 1, dispatch and accept the correctly correlated generation-0 reply. The current generation is still 0, so acceptance is valid.

Full traces and TLC fingerprint-collision estimates are retained in the linked logs. Local parsing paths and process IDs are removed by the runner. These results establish bounded model safety only; atomic storage contracts, implementation refinement and broader limitations are described in [README.md](README.md).
