----------------------------- MODULE Recovery -----------------------------
EXTENDS Naturals, FiniteSets
CONSTANTS Mutation, MaxAttempts, NoticeBudget
Effects == {"emailA", "chatB"}
Sessions == {"sA", "sB"}
Destinations == {"accountA/email/recipient", "accountB/chat/thread"}
Source(e) == IF e = "emailA" THEN "sA" ELSE "sB"
Target(e) == IF e = "emailA" THEN "accountA/email/recipient" ELSE "accountB/chat/thread"
Request(e) == IF e = "emailA" THEN "requestA" ELSE "requestB"
Idempotent == {"chatB"}
Keys == Effects \X (1..MaxAttempts)
Record(e, a, o, s) == [effect |-> e, request |-> Request(e), attempt |-> a,
                       outcome |-> o, source |-> s, destination |-> Target(e)]
Records == [effect : Effects, request : {"requestA", "requestB"},
            attempt : 1..MaxAttempts, outcome : {"confirmedFailed", "unknown"},
            source : Sessions, destination : Destinations]
VARIABLES phase, outcome, attempts, dlq, notices, noticeCount, remaining, spent,
          authorized, noEffectEvidence, effectIdentity, retryOK, recoveredEmail
vars == <<phase, outcome, attempts, dlq, notices, noticeCount, remaining, spent,
          authorized, noEffectEvidence, effectIdentity, retryOK, recoveredEmail>>
Init ==
    /\ phase = [e \in Effects |-> "ready"]
    /\ outcome = [e \in Effects |-> "none"]
    /\ attempts = [e \in Effects |-> 0] /\ dlq = {} /\ notices = {}
    /\ noticeCount = [k \in Keys |-> 0] /\ remaining = NoticeBudget /\ spent = 0
    /\ authorized = {} /\ noEffectEvidence = {}
    /\ effectIdentity = [e \in Effects |-> e]
    /\ retryOK = TRUE /\ recoveredEmail = FALSE
Attempt(e) ==
    /\ phase[e] = "ready" /\ attempts[e] < MaxAttempts
    /\ phase' = [phase EXCEPT ![e] = "inFlight"]
    /\ attempts' = [attempts EXCEPT ![e] = @ + 1]
    /\ outcome' = [outcome EXCEPT ![e] = "none"]
    /\ noEffectEvidence' = noEffectEvidence \ {e}
    /\ UNCHANGED <<dlq, notices, noticeCount, remaining, spent, authorized,
         effectIdentity, retryOK, recoveredEmail>>
Observe(e, result) ==
    /\ phase[e] = "inFlight"
    /\ outcome' = [outcome EXCEPT ![e] = result]
    /\ phase' = [phase EXCEPT ![e] = IF result = "success" THEN "done" ELSE "unresolved"]
    /\ UNCHANGED <<attempts, dlq, notices, noticeCount, remaining, spent,
         authorized, noEffectEvidence, effectIdentity, retryOK, recoveredEmail>>
Quarantine(e) ==
    /\ phase[e] = "unresolved"
    /\ dlq' = dlq \cup {Record(e, attempts[e], outcome[e], Source(e))}
    /\ phase' = [phase EXCEPT ![e] = "quarantined"]
    /\ UNCHANGED <<outcome, attempts, notices, noticeCount, remaining, spent,
         authorized, noEffectEvidence, effectIdentity, retryOK, recoveredEmail>>
Notify(record, success, payloadSession) ==
    LET key == <<record.effect, record.attempt>>
        actualSource == IF Mutation = "wrongSession" THEN payloadSession ELSE record.source
        actualOutcome == IF Mutation = "unknownAsFailed" THEN "confirmedFailed" ELSE record.outcome
        event == Record(record.effect, record.attempt, actualOutcome, actualSource)
    IN /\ record \in dlq /\ remaining > 0
       /\ (Mutation = "duplicateNotice" \/ noticeCount[key] = 0)
       /\ remaining' = IF Mutation = "renewBudget" /\ ~success
                        THEN NoticeBudget ELSE remaining - 1
       /\ spent' = spent + 1
       /\ notices' = IF success THEN notices \cup {event} ELSE notices
       /\ noticeCount' = IF success THEN [noticeCount EXCEPT ![key] = @ + 1]
                         ELSE noticeCount
       /\ UNCHANGED <<phase, outcome, attempts, dlq, authorized, noEffectEvidence,
            effectIdentity, retryOK, recoveredEmail>>
WasNotified(e) == noticeCount[<<e, attempts[e]>>] > 0
AuthorizeRecovery(e) ==
    \* A trusted policy/user decision for this effect, not authority inferred by an agent.
    /\ phase[e] = "quarantined" /\ WasNotified(e) /\ e \notin authorized
    /\ authorized' = authorized \cup {e}
    /\ UNCHANGED <<phase, outcome, attempts, dlq, notices, noticeCount, remaining,
         spent, noEffectEvidence, effectIdentity, retryOK, recoveredEmail>>
ReconcileNoEffect(e) ==
    \* Authoritative adapter evidence that this attempt did not apply externally.
    /\ phase[e] = "quarantined" /\ outcome[e] = "unknown" /\ WasNotified(e)
    /\ e \notin noEffectEvidence
    /\ noEffectEvidence' = noEffectEvidence \cup {e}
    /\ UNCHANGED <<phase, outcome, attempts, dlq, notices, noticeCount, remaining,
         spent, authorized, effectIdentity, retryOK, recoveredEmail>>
SafeRetry(e) == e \in authorized /\
    (outcome[e] = "confirmedFailed" \/ e \in noEffectEvidence \/ e \in Idempotent)
Redrive(e) ==
    /\ phase[e] = "quarantined" /\ attempts[e] < MaxAttempts
    /\ (Mutation = "blindRedrive" \/ SafeRetry(e))
    /\ phase' = [phase EXCEPT ![e] = "ready"]
    /\ retryOK' = (retryOK /\ SafeRetry(e))
    /\ effectIdentity' = [effectIdentity EXCEPT ![e] =
          IF Mutation = "freshIdentity" THEN "replacement" ELSE @]
    /\ recoveredEmail' = (recoveredEmail \/
          (e = "emailA" /\ outcome[e] = "unknown" /\ SafeRetry(e)))
    /\ authorized' = authorized \ {e}
    /\ UNCHANGED <<outcome, attempts, dlq, notices, noticeCount, remaining,
         spent, noEffectEvidence>>
Next == (\E e \in Effects : Attempt(e) \/ Quarantine(e) \/ AuthorizeRecovery(e)
          \/ ReconcileNoEffect(e) \/ Redrive(e)
          \/ (\E result \in {"success", "confirmedFailed", "unknown"} : Observe(e, result)))
     \/ (\E record \in dlq, success \in BOOLEAN, s \in Sessions : Notify(record, success, s))
Spec == Init /\ [][Next]_vars
TypeOK ==
    /\ phase \in [Effects -> {"ready", "inFlight", "done", "unresolved", "quarantined"}]
    /\ outcome \in [Effects -> {"none", "success", "confirmedFailed", "unknown"}]
    /\ attempts \in [Effects -> 0..MaxAttempts] /\ dlq \subseteq Records /\ notices \subseteq Records
    /\ noticeCount \in [Keys -> Nat] /\ remaining \in 0..NoticeBudget /\ spent \in Nat
    /\ authorized \subseteq Effects /\ noEffectEvidence \subseteq Effects
    /\ effectIdentity \in [Effects -> Effects \cup {"replacement"}]
    /\ retryOK \in BOOLEAN /\ recoveredEmail \in BOOLEAN
RecoveryEvidence == notices \subseteq dlq
AtMostOneRecoveryEvent == \A k \in Keys : noticeCount[k] <= 1
BoundedRecoveryNotifications == spent <= NoticeBudget
AuthorizedSafeRedrive == retryOK
StableEffectIdentity == \A e \in Effects : effectIdentity[e] = e
NoUnknownNotification == ~ (\E n \in notices : n.outcome = "unknown")
NoInvestigatedEmailRedrive == ~recoveredEmail
=============================================================================
