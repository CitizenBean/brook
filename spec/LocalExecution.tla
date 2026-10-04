-------------------------- MODULE LocalExecution --------------------------
EXTENDS Naturals, FiniteSets
CONSTANT Mutation
\* One already durably admitted request. Bounded composition of an outbox,
\* authenticated reply/resume intent, logical job, physical attempts and restart.
\* Authorization, SQLite atomicity, source identity and pin contents are assumed.
VARIABLES up, crashes, epoch, send, sends, invoked, externalCount,
          accepted, intent, materialized, job, attempts, issued,
          result, completions, pins, commitOK
vars == <<up, crashes, epoch, send, sends, invoked, externalCount,
          accepted, intent, materialized, job, attempts, issued,
          result, completions, pins, commitOK>>
Init == /\ up = TRUE /\ crashes = 0 /\ epoch = 1
        /\ send = "ready" /\ sends = 0 /\ invoked = FALSE /\ externalCount = 0
        /\ accepted = FALSE /\ intent = FALSE /\ materialized = FALSE
        /\ job = "none" /\ attempts = 0 /\ issued = {}
        /\ result = FALSE /\ completions = 0 /\ pins = TRUE /\ commitOK = TRUE
AdmitSend ==
    /\ up /\ send = "ready" /\ sends < 2
    /\ send' = "inFlight" /\ sends' = sends + 1 /\ invoked' = FALSE
    /\ UNCHANGED <<up, crashes, epoch, externalCount, accepted, intent,
         materialized, job, attempts, issued, result, completions, pins, commitOK>>
ProviderApply ==
    /\ up /\ send = "inFlight" /\ ~invoked
    /\ invoked' = TRUE /\ externalCount' = externalCount + 1
    /\ UNCHANGED <<up, crashes, epoch, send, sends, accepted, intent,
         materialized, job, attempts, issued, result, completions, pins, commitOK>>
RecordSuccess ==
    /\ up /\ send = "inFlight" /\ invoked /\ send' = "sent"
    /\ pins' = IF result THEN FALSE ELSE pins
    /\ UNCHANGED <<up, crashes, epoch, sends, invoked, externalCount,
         accepted, intent, materialized, job, attempts, issued, result,
         completions, commitOK>>
AcceptReply ==
    /\ up /\ externalCount > 0 /\ ~accepted
    /\ accepted' = TRUE /\ intent' = (Mutation # "splitReply")
    /\ UNCHANGED <<up, crashes, epoch, send, sends, invoked, externalCount,
         materialized, job, attempts, issued, result, completions, pins, commitOK>>
Materialize ==
    /\ up /\ intent /\ ~materialized
    /\ materialized' = TRUE
    /\ job' = IF Mutation = "earlyDone" THEN "done" ELSE "ready"
    /\ pins' = IF Mutation = "earlyUnpin" THEN FALSE ELSE pins
    /\ UNCHANGED <<up, crashes, epoch, send, sends, invoked, externalCount,
         accepted, intent, attempts, issued, result, completions, commitOK>>
Claim ==
    /\ up /\ job = "ready" /\ attempts < 2
    /\ job' = "running" /\ attempts' = attempts + 1
    /\ issued' = issued \cup {<<epoch, attempts + 1>>}
    /\ UNCHANGED <<up, crashes, epoch, send, sends, invoked, externalCount,
         accepted, intent, materialized, result, completions, pins, commitOK>>
Complete(token) ==
    /\ up /\ token \in issued
    /\ (job = "running" \/ (Mutation = "repeatCommit" /\ job = "done"))
    /\ (Mutation = "staleAttempt" \/ token = <<epoch, attempts>>)
    /\ job' = "done" /\ result' = TRUE /\ completions' = completions + 1
    /\ pins' = IF Mutation = "terminalUnpin" \/ send = "sent" THEN FALSE ELSE pins
    /\ commitOK' = (commitOK /\ token = <<epoch, attempts>>)
    /\ UNCHANGED <<up, crashes, epoch, send, sends, invoked, externalCount,
         accepted, intent, materialized, attempts, issued>>
Crash ==
    /\ up /\ crashes < 2 /\ up' = FALSE /\ crashes' = crashes + 1
    /\ job' = IF Mutation = "volatileJob" /\ job \in {"ready", "running"}
              THEN "none" ELSE job
    /\ UNCHANGED <<epoch, send, sends, invoked, externalCount, accepted,
         intent, materialized, attempts, issued, result, completions, pins, commitOK>>
Restart ==
    /\ ~up /\ up' = TRUE /\ epoch' = epoch + 1
    /\ send' = IF send = "inFlight" THEN
                   IF Mutation = "blindResend" THEN "ready" ELSE "unknown"
               ELSE send
    /\ job' = IF job = "running" THEN "ready" ELSE job
    /\ UNCHANGED <<crashes, sends, invoked, externalCount, accepted, intent,
         materialized, attempts, issued, result, completions, pins, commitOK>>
Next == AdmitSend \/ ProviderApply \/ RecordSuccess \/ AcceptReply \/ Materialize
     \/ Claim \/ Crash \/ Restart \/ (\E token \in issued : Complete(token))
Spec == Init /\ [][Next]_vars
TypeOK == /\ up \in BOOLEAN /\ crashes \in 0..2 /\ epoch \in 1..3
          /\ send \in {"ready","inFlight","sent","unknown"}
          /\ sends \in 0..2 /\ invoked \in BOOLEAN /\ externalCount \in 0..2
          /\ accepted \in BOOLEAN /\ intent \in BOOLEAN /\ materialized \in BOOLEAN
          /\ job \in {"none","ready","running","done"} /\ attempts \in 0..2
          /\ issued \subseteq (1..3) \X (1..2) /\ result \in BOOLEAN
          /\ completions \in Nat /\ pins \in BOOLEAN /\ commitOK \in BOOLEAN
AtomicReply == accepted => intent
DurableJob == materialized => job # "none"
NoBlindResend == sends <= 1
DoneHasResult == job = "done" => result
CompletionOnce == completions <= 1
FencedAttempt == commitOK
LiveJobPinned == ~result => pins
LiveDependenciesPinned == (~result \/ send \in {"ready", "inFlight", "unknown"}) => pins
=============================================================================
