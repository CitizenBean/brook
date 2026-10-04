-------------------------- MODULE SessionContext --------------------------
EXTENDS Naturals, FiniteSets
CONSTANT Mutation
Sessions == {"sA", "sB"}
Requests == {"a1", "a2", "b1"}
Owner(r) == IF r = "b1" THEN "sB" ELSE "sA"
Other(s) == IF s = "sA" THEN "sB" ELSE "sA"
Base(s) == <<s, "base">>
Fresh(s) == <<s, "fresh">>
Call(r) == <<Owner(r), r, "call">>
Result(r) == <<Owner(r), r, "result">>
Items(s) == {Base(s), Fresh(s)} \cup
    {Call(r) : r \in {q \in Requests : Owner(q) = s}} \cup
    {Result(r) : r \in {q \in Requests : Owner(q) = s}}
AllItems == Items("sA") \cup Items("sB")
Active == {"pending", "accepted"}
VARIABLES history, revision, generation, cancelled, fresh,
          cache, archiveAvailable, status, captured, capturedGen,
          resumeIntents, claims, runs, laterFirst,
          retentionOK, recoveryOK, preserved, ownershipOK, contextOK, compactRecovered, otherCancelResume, laterResumedFirst, requestClosed
vars == <<history, revision, generation, cancelled, fresh,
          cache, archiveAvailable, status, captured, capturedGen,
          resumeIntents, claims, runs, laterFirst,
          retentionOK, recoveryOK, preserved, ownershipOK, contextOK, compactRecovered, otherCancelResume, laterResumedFirst, requestClosed>>
Init ==
    /\ history = [s \in Sessions |-> {Base(s)}]
    /\ revision = [s \in Sessions |-> 0]
    /\ generation = [s \in Sessions |-> 0] /\ cancelled = {} /\ fresh = {}
    /\ cache = [s \in Sessions |-> "full"]
    /\ archiveAvailable = [s \in Sessions |-> TRUE]
    /\ status = [r \in Requests |-> "new"]
    /\ captured = [r \in Requests |-> {}]
    /\ capturedGen = [r \in Requests |-> 0]
    /\ resumeIntents = {}
    /\ claims = [r \in Requests |-> 0] /\ runs = [r \in Requests |-> 0]
    /\ laterFirst = FALSE /\ retentionOK = TRUE /\ recoveryOK = TRUE
    /\ preserved = TRUE /\ ownershipOK = TRUE
    /\ contextOK = TRUE /\ compactRecovered = FALSE
    /\ otherCancelResume = FALSE /\ laterResumedFirst = FALSE
    /\ requestClosed = {}
Prepare(r) ==
    LET s == Owner(r) IN
    /\ status[r] = "new" /\ archiveAvailable[s]
    \* Establish issue order for the out-of-order witness, without ordering replies.
    /\ (r # "a2" \/ status["a1"] # "new")
    /\ history' = [history EXCEPT ![s] = @ \cup {Call(r)}]
    /\ revision' = [revision EXCEPT ![s] = @ + 1]
    /\ status' = [status EXCEPT ![r] = "pending"]
    /\ captured' = [captured EXCEPT ![r] = history[s] \cup {Call(r)}]
    /\ capturedGen' = [capturedGen EXCEPT ![r] = generation[s]]
    /\ UNCHANGED <<generation, cancelled, fresh, cache,
         archiveAvailable, resumeIntents, claims, runs, laterFirst,
         retentionOK, recoveryOK, preserved, ownershipOK, contextOK, compactRecovered, otherCancelResume, laterResumedFirst, requestClosed>>
Message(s) ==
    /\ s \notin fresh
    /\ fresh' = fresh \cup {s}
    /\ history' = [history EXCEPT ![s] = @ \cup {Fresh(s)}]
    /\ revision' = [t \in Sessions |->
          IF t = s \/ Mutation = "globalRevision" THEN revision[t] + 1
          ELSE revision[t]]
        /\ UNCHANGED <<generation, cancelled, cache, archiveAvailable, status,
         captured, capturedGen, resumeIntents, claims, runs, laterFirst,
         retentionOK, recoveryOK, preserved, ownershipOK, contextOK, compactRecovered, otherCancelResume, laterResumedFirst, requestClosed>>
Visible(s) == CASE cache[s] = "full" -> history[s]
                   [] cache[s] = "compact" -> history[s] \cap {Fresh(s)}
                   [] OTHER -> {}
Compact(s) ==
    /\ cache[s] # "compact"
    /\ cache' = [cache EXCEPT ![s] = "compact"]
    /\ UNCHANGED <<history, revision, generation, cancelled, fresh,
         archiveAvailable, status, captured, capturedGen, resumeIntents,
         claims, runs, laterFirst, retentionOK, recoveryOK, preserved, ownershipOK, contextOK, compactRecovered, otherCancelResume, laterResumedFirst, requestClosed>>
Evict(s) ==
    /\ cache[s] # "empty"
    /\ cache' = [cache EXCEPT ![s] = "empty"]
    /\ UNCHANGED <<history, revision, generation, cancelled, fresh,
         archiveAvailable, status, captured, capturedGen, resumeIntents,
         claims, runs, laterFirst, retentionOK, recoveryOK, preserved, ownershipOK, contextOK, compactRecovered, otherCancelResume, laterResumedFirst, requestClosed>>
Pinned(s) == \E r \in Requests : Owner(r) = s /\ status[r] \in Active
Reclaim(s) ==
    /\ archiveAvailable[s]
    /\ (Mutation = "unpin" \/ ~Pinned(s))
    /\ archiveAvailable' = [archiveAvailable EXCEPT ![s] = FALSE]
    /\ retentionOK' = (retentionOK /\ ~Pinned(s))
    /\ UNCHANGED <<history, revision, generation, cancelled, fresh,
         cache, status, captured, capturedGen, resumeIntents,
         claims, runs, laterFirst, recoveryOK, preserved, ownershipOK, contextOK, compactRecovered, otherCancelResume, laterResumedFirst, requestClosed>>
LoseArchive(s) ==
    \* Explicit environmental storage-loss fault, distinct from normal reclamation.
    /\ archiveAvailable[s]
    /\ archiveAvailable' = [archiveAvailable EXCEPT ![s] = FALSE]
    /\ UNCHANGED <<history, revision, generation, cancelled, fresh,
         cache, status, captured, capturedGen, resumeIntents,
         claims, runs, laterFirst, retentionOK, recoveryOK, preserved, ownershipOK, contextOK, compactRecovered, otherCancelResume, laterResumedFirst, requestClosed>>
Accept(r, replyOwner) ==
    LET s == Owner(r)
        actual == IF Mutation = "wrongReturn" THEN replyOwner ELSE s
    IN /\ status[r] = "pending" /\ capturedGen[r] = generation[s]
       /\ (Mutation = "wrongReturn" \/ replyOwner = s)
       /\ status' = [status EXCEPT ![r] = "accepted"]
       /\ history' = [history EXCEPT ![actual] = @ \cup {Result(r)}]
       /\ revision' = [revision EXCEPT ![actual] = @ + 1]
       /\ resumeIntents' = resumeIntents \cup {r}
       /\ claims' = [claims EXCEPT ![r] = @ + 1]
       /\ ownershipOK' = (ownershipOK /\ actual = s)
       /\ laterFirst' = (laterFirst \/
              (r = "a2" /\ status["a1"] = "pending" /\ status["b1"] \in Active))
       /\ UNCHANGED <<generation, cancelled, fresh, cache,
            archiveAvailable, captured, capturedGen, runs,
            retentionOK, recoveryOK, preserved, contextOK, compactRecovered, otherCancelResume, laterResumedFirst, requestClosed>>
Resume(r) ==
    LET s == Owner(r)
        restored == IF Mutation = "copyOther" THEN history[Other(s)]
                    ELSE IF Mutation = "rollback" THEN captured[r] \cup {Result(r)}
                    ELSE history[s] \cup captured[r]
    IN /\ status[r] = "accepted" /\ r \in resumeIntents
       /\ capturedGen[r] = generation[s]
       /\ (Mutation = "fabricate" \/ archiveAvailable[s])
       /\ status' = [status EXCEPT ![r] = "done"]
       /\ runs' = [runs EXCEPT ![r] = @ + 1]
       /\ cache' = [cache EXCEPT ![s] = "full"]
       /\ contextOK' = (contextOK /\ restored \subseteq Items(s))
       /\ compactRecovered' = (compactRecovered \/
            (cache[s] = "compact" /\ Call(r) \notin Visible(s)))
       /\ otherCancelResume' = (otherCancelResume \/
            (s = "sB" /\ "sA" \in cancelled))
       /\ laterResumedFirst' = (laterResumedFirst \/
            (r = "a2" /\ status["a1"] \in Active))
       /\ recoveryOK' = (recoveryOK /\ archiveAvailable[s])
       /\ preserved' = (preserved /\ history[s] \subseteq restored
                         /\ captured[r] \cup {Result(r)} \subseteq restored)
       /\ UNCHANGED <<history, revision, generation, cancelled, fresh,
            archiveAvailable, captured, capturedGen, resumeIntents, claims,
            laterFirst, retentionOK, ownershipOK, requestClosed>>
RecoveryFailed(r) ==
    /\ status[r] = "accepted" /\ ~archiveAvailable[Owner(r)]
    /\ status' = [status EXCEPT ![r] = "recoveryFailed"]
    /\ UNCHANGED <<history, revision, generation, cancelled, fresh,
         cache, archiveAvailable, captured, capturedGen,
         resumeIntents, claims, runs, laterFirst, retentionOK,
         recoveryOK, preserved, ownershipOK, contextOK, compactRecovered, otherCancelResume, laterResumedFirst, requestClosed>>
Cancel(s) ==
    /\ generation[s] = 0
    /\ cancelled' = cancelled \cup {s}
    /\ generation' = [t \in Sessions |->
          IF t = s \/ Mutation = "globalCancel" THEN 1 ELSE generation[t]]
    /\ status' = [r \in Requests |->
          IF Owner(r) = s /\ status[r] \in Active THEN "cancelled" ELSE status[r]]
    /\ UNCHANGED <<history, revision, fresh, cache, archiveAvailable,
         captured, capturedGen, resumeIntents, claims, runs, laterFirst,
         retentionOK, recoveryOK, preserved, ownershipOK, contextOK, compactRecovered, otherCancelResume, laterResumedFirst, requestClosed>>
CloseRequest(r) ==
    /\ status[r] \in Active
    /\ status' = [q \in Requests |->
          IF (q = r \/ Mutation = "closeOthers") /\ status[q] \in Active
          THEN "cancelled" ELSE status[q]]
    /\ requestClosed' = requestClosed \cup {r}
    /\ UNCHANGED <<history, revision, generation, cancelled, fresh, cache,
         archiveAvailable, captured, capturedGen, resumeIntents, claims, runs,
         laterFirst, retentionOK, recoveryOK, preserved, ownershipOK,
         contextOK, compactRecovered, otherCancelResume, laterResumedFirst>>
Next == (\E r \in Requests : Prepare(r) \/ Resume(r) \/ RecoveryFailed(r) \/ CloseRequest(r)
             \/ (\E s \in Sessions : Accept(r, s)))
     \/ (\E s \in Sessions : Message(s) \/ Compact(s) \/ Evict(s)
                          \/ Reclaim(s) \/ LoseArchive(s) \/ Cancel(s))
Spec == Init /\ [][Next]_vars
TypeOK ==
    /\ history \in [Sessions -> SUBSET AllItems]
    /\ cache \in [Sessions -> {"full", "compact", "empty"}]
    /\ revision \in [Sessions -> 0..8] /\ generation \in [Sessions -> 0..1]
    /\ cancelled \subseteq Sessions /\ fresh \subseteq Sessions
    /\ requestClosed \subseteq Requests
    /\ archiveAvailable \in [Sessions -> BOOLEAN]
    /\ status \in [Requests -> {"new", "pending", "accepted", "done",
                                "cancelled", "recoveryFailed"}]
    /\ captured \in [Requests -> SUBSET AllItems]
    /\ capturedGen \in [Requests -> 0..1]
    /\ resumeIntents \subseteq Requests
    /\ claims \in [Requests -> 0..1] /\ runs \in [Requests -> 0..1]
    /\ laterFirst \in BOOLEAN /\ retentionOK \in BOOLEAN /\ recoveryOK \in BOOLEAN
    /\ preserved \in BOOLEAN /\ ownershipOK \in BOOLEAN
    /\ contextOK \in BOOLEAN /\ compactRecovered \in BOOLEAN
    /\ otherCancelResume \in BOOLEAN /\ laterResumedFirst \in BOOLEAN
HistoryIsolation == \A s \in Sessions : history[s] \subseteq Items(s)
ContextIsolation == contextOK
CapturedIsolation == \A r \in Requests : captured[r] \subseteq Items(Owner(r))
SessionLocalRevision == \A s \in Sessions : revision[s] = Cardinality(history[s]) - 1
SessionLocalCancellation == \A s \in Sessions :
    generation[s] = IF s \in cancelled THEN 1 ELSE 0
RequestLocalClosure == \A r \in Requests : status[r] = "cancelled" =>
    (r \in requestClosed \/ capturedGen[r] # generation[Owner(r)])
OwnedReturn == ownershipOK
RetainedWhilePending == retentionOK
NoFabricatedRecovery == recoveryOK
PreserveCausalAndNewer == preserved
AtomicResumeIntent == \A r \in Requests : claims[r] = 1 <=> r \in resumeIntents
NoOutOfOrderCompletion == ~(laterFirst /\ laterResumedFirst /\ \A r \in Requests : status[r] = "done")
NoOtherCancelResume == ~otherCancelResume
NoCompactedRecovery == ~compactRecovered
NoRecoveryFailure == \A r \in Requests : status[r] # "recoveryFailed"
=============================================================================
