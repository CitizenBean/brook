-------------------------- MODULE ProcessorCommit --------------------------
EXTENDS Naturals, FiniteSets, TLC
CONSTANT Mutation
VARIABLES admitted, receipts, prepared, generation, snapshotGeneration,
          revision, snapshotRevision, done, result, stateCommitted, children,
          commits, authorized, validRoute, quota, acceptedFence, acceptedState,
          acceptedAuthority, acceptedRoute, printState, writes
vars == <<admitted, receipts, prepared, generation, snapshotGeneration,
          revision, snapshotRevision, done, result, stateCommitted, children,
          commits, authorized, validRoute, quota, acceptedFence, acceptedState,
          acceptedAuthority, acceptedRoute, printState, writes>>
Init == /\ admitted = FALSE /\ receipts = 0 /\ prepared = FALSE
        /\ generation = 1 /\ snapshotGeneration = 0
        /\ revision = 0 /\ snapshotRevision = 0
        /\ done = FALSE /\ result = FALSE /\ stateCommitted = FALSE
        /\ children = {} /\ commits = 0
        /\ authorized = TRUE /\ validRoute = TRUE /\ quota = 2
        /\ acceptedFence = TRUE /\ acceptedState = TRUE
        /\ acceptedAuthority = TRUE /\ acceptedRoute = TRUE
        /\ printState = "pending" /\ writes = 0
Admit == /\ ~admitted /\ admitted' = TRUE /\ receipts' = 1
         /\ UNCHANGED <<prepared, generation, snapshotGeneration, revision,
              snapshotRevision, done, result, stateCommitted, children, commits,
              authorized, validRoute, quota, acceptedFence, acceptedState,
              acceptedAuthority, acceptedRoute, printState, writes>>
Duplicate == /\ admitted /\ receipts < 2 /\ Mutation = "duplicate"
             /\ receipts' = receipts + 1
             /\ UNCHANGED <<admitted, prepared, generation, snapshotGeneration,
                  revision, snapshotRevision, done, result, stateCommitted,
                  children, commits, authorized, validRoute, quota, acceptedFence,
                  acceptedState, acceptedAuthority, acceptedRoute, printState, writes>>
Prepare == /\ admitted /\ ~prepared /\ ~done
           /\ prepared' = TRUE /\ snapshotGeneration' = generation
           /\ snapshotRevision' = revision
           /\ authorized' \in BOOLEAN /\ validRoute' \in BOOLEAN /\ quota' \in {1,2}
           /\ UNCHANGED <<admitted, receipts, generation, revision, done, result,
                stateCommitted, children, commits, acceptedFence, acceptedState,
                acceptedAuthority, acceptedRoute, printState, writes>>
Takeover == /\ prepared /\ ~done /\ generation = 1 /\ generation' = 2
            /\ UNCHANGED <<admitted, receipts, prepared, snapshotGeneration,
                 revision, snapshotRevision, done, result, stateCommitted,
                 children, commits, authorized, validRoute, quota, acceptedFence,
                 acceptedState, acceptedAuthority, acceptedRoute, printState, writes>>
OtherStateCommit == /\ prepared /\ ~done /\ revision = 0 /\ revision' = 1
                   /\ UNCHANGED <<admitted, receipts, prepared, generation,
                        snapshotGeneration, snapshotRevision, done, result,
                        stateCommitted, children, commits, authorized, validRoute,
                        quota, acceptedFence, acceptedState, acceptedAuthority,
                        acceptedRoute, printState, writes>>
Commit == /\ prepared /\ ~done
          /\ (snapshotGeneration = generation \/ Mutation = "staleOwner")
          /\ (snapshotRevision = revision \/ Mutation = "staleState")
          /\ (authorized \/ Mutation = "unauthorized")
          /\ (validRoute \/ Mutation = "unknownRoute")
          /\ (quota >= 2 \/ Mutation = "overQuota")
          /\ done' = TRUE /\ result' = (Mutation # "omitResult")
          /\ stateCommitted' = (Mutation # "omitState")
          /\ children' = IF Mutation = "partialFanout" THEN {"a"} ELSE {"a","b"}
          /\ commits' = commits + 1
          /\ acceptedFence' = (snapshotGeneration = generation)
          /\ acceptedState' = (snapshotRevision = revision)
          /\ acceptedAuthority' = authorized /\ acceptedRoute' = validRoute
          /\ UNCHANGED <<admitted, receipts, prepared, generation,
               snapshotGeneration, revision, snapshotRevision, authorized,
               validRoute, quota, printState, writes>>
RepeatCommit == /\ done /\ commits = 1 /\ Mutation = "repeatCommit"
                /\ commits' = 2
                /\ UNCHANGED <<admitted, receipts, prepared, generation,
                     snapshotGeneration, revision, snapshotRevision, done,
                     result, stateCommitted, children, authorized, validRoute,
                     quota, acceptedFence, acceptedState, acceptedAuthority,
                     acceptedRoute, printState, writes>>
BeginPrint == /\ done /\ printState = "pending" /\ printState' = "printing"
              /\ UNCHANGED <<admitted, receipts, prepared, generation,
                   snapshotGeneration, revision, snapshotRevision, done, result,
                   stateCommitted, children, commits, authorized, validRoute,
                   quota, acceptedFence, acceptedState, acceptedAuthority, acceptedRoute, writes>>
ConsumeWrite == /\ printState = "printing" /\ printState' = "write_started"
                /\ UNCHANGED <<admitted, receipts, prepared, generation,
                     snapshotGeneration, revision, snapshotRevision, done, result,
                     stateCommitted, children, commits, authorized, validRoute,
                     quota, acceptedFence, acceptedState, acceptedAuthority, acceptedRoute, writes>>
Write == /\ printState = "write_started" /\ writes = 0
         /\ writes' = 1
         /\ UNCHANGED <<admitted, receipts, prepared, generation,
              snapshotGeneration, revision, snapshotRevision, done, result,
              stateCommitted, children, commits, authorized, validRoute,
              quota, acceptedFence, acceptedState, acceptedAuthority, acceptedRoute, printState>>
FailedFinalizationRetry == /\ printState = "write_started" /\ writes = 1
                           /\ Mutation = "repeatWrite" /\ writes' = 2
                           /\ UNCHANGED <<admitted, receipts, prepared, generation,
                                snapshotGeneration, revision, snapshotRevision, done, result,
                                stateCommitted, children, commits, authorized, validRoute,
                                quota, acceptedFence, acceptedState, acceptedAuthority, acceptedRoute, printState>>
CrashPrint == /\ printState \in {"printing", "write_started"} /\ printState' = "unknown"
              /\ UNCHANGED <<admitted, receipts, prepared, generation,
                   snapshotGeneration, revision, snapshotRevision, done, result,
                   stateCommitted, children, commits, authorized, validRoute,
                   quota, acceptedFence, acceptedState, acceptedAuthority, acceptedRoute, writes>>
BlindResend == /\ printState = "unknown" /\ Mutation = "blindResend"
               /\ printState' = "resent"
               /\ UNCHANGED <<admitted, receipts, prepared, generation,
                    snapshotGeneration, revision, snapshotRevision, done, result,
                    stateCommitted, children, commits, authorized, validRoute,
                    quota, acceptedFence, acceptedState, acceptedAuthority, acceptedRoute, writes>>
Next == Admit \/ Duplicate \/ Prepare \/ Takeover \/ OtherStateCommit \/ Commit
        \/ RepeatCommit \/ BeginPrint \/ ConsumeWrite \/ Write \/ FailedFinalizationRetry \/ CrashPrint \/ BlindResend
TypeOK == /\ receipts \in 0..2 /\ commits \in 0..2 /\ children \subseteq {"a","b"}
          /\ generation \in 1..2 /\ revision \in 0..1
          /\ printState \in {"pending","printing","write_started","unknown","resent"}
          /\ writes \in 0..2
DeduplicatedAdmission == receipts <= 1
AtomicFanout == done => children = {"a","b"}
AtomicResult == done => result
AtomicState == done => stateCommitted
CompletionOnce == commits <= 1
FencedOwner == acceptedFence
FreshState == acceptedState
AuthorizedRoutes == acceptedAuthority
KnownRoutes == acceptedRoute
BoundedFanout == Cardinality(children) <= quota
NoBlindResend == printState # "resent"
SingleWrite == writes <= 1
Spec == Init /\ [][Next]_vars
=============================================================================
