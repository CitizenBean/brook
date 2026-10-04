------------------------ MODULE Continuations ------------------------
EXTENDS Naturals, FiniteSets
CONSTANTS Requests, MaxRevision, MaxGeneration, MaxAttempts, Mutation
VARIABLES checkpoint, request, outbox, sent, status, capturedRev, capturedGen,
          generation, revision, activity, context, replies, resumes, runs, claims, calls, results,
          attempts, effect, acceptedValid, resumeValid, preserved, advancedReply
vars == <<checkpoint, request, outbox, sent, status, capturedRev, capturedGen,
          generation, revision, activity, context, replies, resumes, runs, claims, calls, results,
          attempts, effect, acceptedValid, resumeValid, preserved, advancedReply>>
Init == /\ checkpoint = {} /\ request = {} /\ outbox = {} /\ sent = {}
        /\ status = [r \in Requests |-> "new"]
        /\ capturedRev = [r \in Requests |-> 0]
        /\ capturedGen = [r \in Requests |-> 0]
        /\ generation = 0 /\ revision = 0 /\ activity = {} /\ context = {}
        /\ replies = {} /\ resumes = {}
        /\ claims = [r \in Requests |-> 0]
        /\ calls = {} /\ results = {}
        /\ runs = [r \in Requests |-> 0]
        /\ attempts = [r \in Requests |-> 0]
        /\ effect = [r \in Requests |-> r]
        /\ acceptedValid = TRUE /\ resumeValid = TRUE /\ preserved = TRUE /\ advancedReply = FALSE
Prepare(r) ==
    /\ status[r] = "new"
    /\ checkpoint' = checkpoint \cup {r}
    /\ request' = request \cup {r} /\ outbox' = outbox \cup {r}
    /\ status' = [status EXCEPT ![r] = "pending"]
    /\ capturedRev' = [capturedRev EXCEPT ![r] = revision]
    /\ capturedGen' = [capturedGen EXCEPT ![r] = generation]
    /\ UNCHANGED <<sent, generation, revision, activity, context, replies,
                    resumes, runs, claims, calls, results, attempts, effect, acceptedValid,
                    resumeValid, preserved, advancedReply>>
Dispatch(r) ==
    /\ IF Mutation = "earlyDispatch" THEN r \notin sent
       ELSE r \in outbox /\ r \notin sent
    /\ sent' = sent \cup {r}
    /\ attempts' = [attempts EXCEPT ![r] = 1]
    /\ UNCHANGED <<checkpoint, request, outbox, status, capturedRev,
         capturedGen, generation, revision, activity, context, replies,
         resumes, runs, claims, calls, results, effect, acceptedValid, resumeValid, preserved, advancedReply>>
Retry(r) ==
    /\ r \in sent /\ attempts[r] < MaxAttempts
    /\ attempts' = [attempts EXCEPT ![r] = @ + 1]
    /\ effect' = [effect EXCEPT ![r] =
          IF Mutation = "retryIdentity" THEN "renamed" ELSE @]
    /\ UNCHANGED <<checkpoint, request, outbox, sent, status, capturedRev,
         capturedGen, generation, revision, activity, context, replies,
         resumes, runs, claims, calls, results, acceptedValid, resumeValid, preserved, advancedReply>>
Advance ==
    /\ revision < MaxRevision
    /\ revision' = revision + 1
    /\ activity' = activity \cup {revision + 1}
    /\ context' = context \cup {revision + 1}
    /\ UNCHANGED <<checkpoint, request, outbox, sent, status, capturedRev,
         capturedGen, generation, replies, resumes, runs, claims, calls, results, attempts, effect,
         acceptedValid, resumeValid, preserved, advancedReply>>
Cancel ==
    /\ generation < MaxGeneration /\ generation' = generation + 1
    /\ UNCHANGED <<checkpoint, request, outbox, sent, status, capturedRev,
         capturedGen, revision, activity, context, replies, resumes, runs, claims, calls, results,
         attempts, effect, acceptedValid, resumeValid, preserved, advancedReply>>
Close(r) ==
    /\ status[r] = "pending"
    /\ status' = [status EXCEPT ![r] = "closed"]
    /\ UNCHANGED <<checkpoint, request, outbox, sent, capturedRev,
         capturedGen, generation, revision, activity, context, replies,
         resumes, runs, claims, calls, results, attempts, effect, acceptedValid, resumeValid, preserved, advancedReply>>
\* Arbitrarily many deliveries, including wrong correlation/generation/shape.
Accept(r, correlated, replyGen, shapeOK) ==
    /\ r \in sent /\ (Mutation = "correlation" \/ correlated = r) /\ shapeOK
    /\ (status[r] = "pending" \/
          (Mutation = "duplicate" /\ status[r] \in {"accepted", "done"}))
    /\ replyGen = capturedGen[r]
    /\ (Mutation = "stale" \/ capturedGen[r] = generation)
    /\ status' = [status EXCEPT ![r] = "accepted"]
    /\ replies' = replies \cup {r}
    /\ claims' = [claims EXCEPT ![r] = @ + 1]
    /\ advancedReply' = (advancedReply \/ capturedRev[r] < revision)
    /\ resumes' = IF Mutation = "splitAccept" THEN resumes
                    ELSE resumes \cup {r}
    /\ acceptedValid' = (acceptedValid /\ correlated = r /\ status[r] = "pending"
               /\ replyGen = capturedGen[r] /\ capturedGen[r] = generation)
    /\ UNCHANGED <<checkpoint, request, outbox, sent, capturedRev,
         capturedGen, generation, revision, activity, context, runs, calls, results,
         attempts, effect, resumeValid, preserved>>
Resume(r) ==
    /\ r \in resumes /\ status[r] = "accepted"
    /\ (Mutation = "staleResume" \/ capturedGen[r] = generation)
    /\ status' = [status EXCEPT ![r] = "done"]
    /\ runs' = [runs EXCEPT ![r] = @ + 1]
    /\ calls' = IF Mutation = "missingCall" THEN calls ELSE calls \cup {r}
    /\ results' = results \cup {r}
    /\ context' = IF Mutation = "overwrite" THEN 1..capturedRev[r]
                   ELSE activity \cup (1..capturedRev[r])
    /\ preserved' = (preserved /\ activity \subseteq context')
    /\ resumeValid' = (resumeValid /\ capturedGen[r] = generation)
    /\ UNCHANGED <<checkpoint, request, outbox, sent, capturedRev,
         capturedGen, generation, revision, activity, replies, resumes,
         attempts, effect, claims, acceptedValid, advancedReply>>
\* Losing a cached projection does not lose durable activity or checkpoints.
Evict == /\ context # {} /\ context' = {}
         /\ UNCHANGED <<checkpoint, request, outbox, sent, status, capturedRev,
              capturedGen, generation, revision, activity, replies, resumes,
              runs, claims, calls, results, attempts, effect, acceptedValid, resumeValid, preserved, advancedReply>>
Next == Advance \/ Cancel \/ Evict \/
    (\E r \in Requests : Prepare(r) \/ Dispatch(r) \/ Retry(r) \/ Close(r)
      \/ Resume(r) \/ (\E c \in Requests \cup {"unknown"},
                            g \in 0..MaxGeneration, s \in BOOLEAN :
                             Accept(r, c, g, s)))
Spec == Init /\ [][Next]_vars
TypeOK ==
    /\ checkpoint \subseteq Requests /\ request \subseteq Requests
    /\ outbox \subseteq Requests /\ sent \subseteq Requests
    /\ status \in [Requests -> {"new", "pending", "closed", "accepted", "done"}]
    /\ capturedRev \in [Requests -> 0..MaxRevision]
    /\ capturedGen \in [Requests -> 0..MaxGeneration]
    /\ generation \in 0..MaxGeneration /\ revision \in 0..MaxRevision
    /\ activity \subseteq 1..MaxRevision /\ context \subseteq 1..MaxRevision
    /\ replies \subseteq Requests /\ resumes \subseteq Requests
    /\ claims \in [Requests -> Nat]
    /\ calls \subseteq Requests /\ results \subseteq Requests
    /\ runs \in [Requests -> Nat] /\ attempts \in [Requests -> 0..MaxAttempts]
    /\ effect \in [Requests -> Requests \cup {"renamed"}]
    /\ acceptedValid \in BOOLEAN /\ resumeValid \in BOOLEAN /\ preserved \in BOOLEAN /\ advancedReply \in BOOLEAN
RecoveryBeforeDispatch == sent \subseteq (checkpoint \cap request \cap outbox)
AtomicAcceptance == replies = resumes
ValidAcceptance == acceptedValid
ValidResume == resumeValid
NoAdvancedReplyAccepted == ~advancedReply
ToolCallResultPairing == calls = results
AtMostOneClaim == \A r \in Requests : claims[r] <= 1
AtMostOneResume == \A r \in Requests : runs[r] <= 1
PreserveNewerActivity == preserved
StableEffectIdentity == \A r \in Requests : effect[r] = r
=============================================================================
