----------------------------- MODULE Admission -----------------------------
EXTENDS Naturals, FiniteSets
CONSTANTS Mutation, Durable
\* Two authorized session scopes reuse the same caller-supplied operation ID.
Keys == {"scopeA/op1", "scopeB/op1"}
Payloads == {"p", "q"}
Tokens == Keys \X Payloads \X (1..2)
DefaultToken == <<"scopeA/op1", "p", 1>>
VARIABLES requests, causal, pending, outbox, committed,
          phase, inputKey, inputPayload, receipt, workQueue,
          observedReceipts, commitHistory, rejected, lostAtCrash,
          committedAtCrash, recoveredReceipt, recoveredDispatch, recoveryOK, admissionCount
vars == <<requests, causal, pending, outbox, committed,
          phase, inputKey, inputPayload, receipt, workQueue,
          observedReceipts, commitHistory, rejected, lostAtCrash,
          committedAtCrash, recoveredReceipt, recoveredDispatch, recoveryOK, admissionCount>>
Complete == requests \cap causal \cap pending \cap outbox
LookupKey == IF Mutation = "omitScope" THEN "scopeA/op1" ELSE inputKey
Existing == {t \in committed : t[1] = LookupKey}
Matching == {t \in Existing : t[2] = inputPayload}
Init ==
    /\ requests = {} /\ causal = {} /\ pending = {} /\ outbox = {} /\ committed = {}
    /\ phase = "idle" /\ inputKey = "scopeA/op1" /\ inputPayload = "p"
    /\ receipt = DefaultToken /\ workQueue = {}
    /\ observedReceipts = {} /\ commitHistory = {} /\ rejected = {}
    /\ lostAtCrash = {} /\ committedAtCrash = {}
    /\ recoveredReceipt = FALSE /\ recoveredDispatch = FALSE /\ recoveryOK = TRUE
    /\ admissionCount = [k \in Keys |-> 0]
Submit(k, p) ==
    /\ phase = "idle"
    /\ phase' = "prepared" /\ inputKey' = k /\ inputPayload' = p
    /\ UNCHANGED <<requests, causal, pending, outbox, committed, receipt, workQueue,
         observedReceipts, commitHistory, rejected, lostAtCrash, committedAtCrash,
         recoveredReceipt, recoveredDispatch, recoveryOK, admissionCount>>
Commit ==
    LET duplicate == Mutation = "retryDuplicate" /\ Matching # {}
        replace == Mutation = "replaceMismatch" /\ Existing # {} /\ Matching = {}
        token == <<LookupKey, inputPayload, IF duplicate THEN 2 ELSE 1>>
        removed == IF replace THEN Existing ELSE {}
    IN /\ phase = "prepared"
       /\ (Existing = {} \/ duplicate \/ replace) /\ token \notin committed
       \* The admission marker and all recovery records share the atomic contract.
       /\ requests' = (requests \ removed) \cup {token}
       /\ committed' = (committed \ removed) \cup {token}
       /\ causal' = IF Mutation = "splitCommit" THEN causal
                    ELSE (causal \ removed) \cup {token}
       /\ pending' = IF Mutation = "splitCommit" THEN pending
                     ELSE (pending \ removed) \cup {token}
       /\ outbox' = IF Mutation = "splitCommit" THEN outbox
                    ELSE (outbox \ removed) \cup {token}
       /\ receipt' = token /\ phase' = "ready"
       /\ workQueue' = workQueue \cup {token}
       /\ commitHistory' = commitHistory \cup {token}
       /\ admissionCount' = [admissionCount EXCEPT ![LookupKey] = @ + 1]
       /\ UNCHANGED <<inputKey, inputPayload, observedReceipts, rejected,
            lostAtCrash, committedAtCrash, recoveredReceipt, recoveredDispatch,
            recoveryOK>>
Reuse ==
    LET choices == IF Mutation = "ignorePayload" THEN Existing ELSE Matching
    IN /\ phase = "prepared" /\ choices # {}
       /\ Mutation # "retryDuplicate"
       /\ receipt' \in choices /\ phase' = "ready"
       /\ UNCHANGED <<requests, causal, pending, outbox, committed, inputKey,
            inputPayload, workQueue, observedReceipts, commitHistory, rejected,
            lostAtCrash, committedAtCrash, recoveredReceipt, recoveredDispatch,
            recoveryOK, admissionCount>>
RejectMismatch ==
    /\ phase = "prepared" /\ Existing # {} /\ Matching = {}
    /\ phase' = "idle"
    /\ rejected' = rejected \cup {<<inputKey, inputPayload>>}
    /\ UNCHANGED <<requests, causal, pending, outbox, committed, inputKey,
         inputPayload, receipt, workQueue, observedReceipts, commitHistory,
         lostAtCrash, committedAtCrash, recoveredReceipt, recoveredDispatch,
         recoveryOK, admissionCount>>
Acknowledge ==
    /\ phase = "ready"
    /\ observedReceipts' = observedReceipts \cup {<<inputKey, inputPayload, receipt>>}
    /\ recoveredReceipt' = (recoveredReceipt \/ receipt \in lostAtCrash)
    /\ phase' = "idle"
    /\ UNCHANGED <<requests, causal, pending, outbox, committed, inputKey,
         inputPayload, receipt, workQueue, commitHistory, rejected, lostAtCrash,
         committedAtCrash, recoveredDispatch, recoveryOK, admissionCount>>
EarlyReceipt ==
    /\ Mutation = "earlyReceipt" /\ phase = "prepared"
    /\ observedReceipts' = observedReceipts \cup
          {<<inputKey, inputPayload, <<inputKey, inputPayload, 1>>>>}
    /\ phase' = "idle"
    /\ UNCHANGED <<requests, causal, pending, outbox, committed, inputKey,
         inputPayload, receipt, workQueue, commitHistory, rejected, lostAtCrash,
         committedAtCrash, recoveredReceipt, recoveredDispatch, recoveryOK, admissionCount>>
LoseReceipt ==
    /\ phase = "ready" /\ phase' = "idle"
    /\ UNCHANGED <<requests, causal, pending, outbox, committed, inputKey,
         inputPayload, receipt, workQueue, observedReceipts, commitHistory,
         rejected, lostAtCrash, committedAtCrash, recoveredReceipt,
         recoveredDispatch, recoveryOK, admissionCount>>
Crash ==
    /\ phase # "down" /\ phase' = "down"
    /\ lostAtCrash' = IF phase = "ready" THEN lostAtCrash \cup {receipt}
                      ELSE lostAtCrash
    /\ committedAtCrash' = committedAtCrash \cup committed
    /\ requests' = IF Durable THEN requests ELSE {}
    /\ causal' = IF Durable /\ Mutation # "volatileCausal" THEN causal ELSE {}
    /\ pending' = IF Durable THEN pending ELSE {}
    /\ outbox' = IF Durable THEN outbox ELSE {}
    /\ committed' = IF Durable THEN committed ELSE {}
    \* All process-local preparation, queued work and unsent receipt state is lost.
    /\ inputKey' = "scopeA/op1" /\ inputPayload' = "p"
    /\ receipt' = DefaultToken /\ workQueue' = {}
    /\ UNCHANGED <<observedReceipts, commitHistory, rejected,
         recoveredReceipt, recoveredDispatch, recoveryOK, admissionCount>>
Restart ==
    /\ phase = "down" /\ phase' = "idle"
    /\ workQueue' = IF Mutation = "dropRecovery" THEN {} ELSE committed \cap Complete
    /\ recoveryOK' = (recoveryOK /\ committed \subseteq workQueue')
    /\ UNCHANGED <<requests, causal, pending, outbox, committed, inputKey,
         inputPayload, receipt, observedReceipts, commitHistory, rejected,
         lostAtCrash, committedAtCrash, recoveredReceipt, recoveredDispatch, admissionCount>>
Dispatch(t) ==
    /\ phase # "down" /\ t \in workQueue /\ t \in committed \cap Complete
    /\ workQueue' = workQueue \ {t}
    /\ recoveredDispatch' = (recoveredDispatch \/ t \in committedAtCrash)
    /\ UNCHANGED <<requests, causal, pending, outbox, committed, phase, inputKey,
         inputPayload, receipt, observedReceipts, commitHistory, rejected,
         lostAtCrash, committedAtCrash, recoveredReceipt, recoveryOK, admissionCount>>
Next == (\E k \in Keys, p \in Payloads : Submit(k, p))
     \/ Commit \/ Reuse \/ RejectMismatch \/ Acknowledge \/ EarlyReceipt
     \/ LoseReceipt \/ Crash \/ Restart \/ (\E t \in Tokens : Dispatch(t))
Spec == Init /\ [][Next]_vars
TypeOK ==
    /\ requests \subseteq Tokens /\ causal \subseteq Tokens
    /\ pending \subseteq Tokens /\ outbox \subseteq Tokens /\ committed \subseteq Tokens
    /\ phase \in {"idle", "prepared", "ready", "down"}
    /\ inputKey \in Keys /\ inputPayload \in Payloads /\ receipt \in Tokens
    /\ workQueue \subseteq Tokens /\ commitHistory \subseteq Tokens
    /\ observedReceipts \subseteq (Keys \X Payloads \X Tokens)
    /\ rejected \subseteq (Keys \X Payloads)
    /\ lostAtCrash \subseteq Tokens /\ committedAtCrash \subseteq Tokens
    /\ recoveredReceipt \in BOOLEAN /\ recoveredDispatch \in BOOLEAN
    /\ recoveryOK \in BOOLEAN /\ admissionCount \in [Keys -> Nat]
CompleteAdmission == committed \subseteq Complete
ReceiptRecoverable == \A r \in observedReceipts : r[3] \in committed \cap Complete
ReceiptMatchesCaller == \A r \in observedReceipts : r[1] = r[3][1] /\ r[2] = r[3][2]
OneLogicalAdmission == \A k \in Keys : admissionCount[k] <= 1
    /\ Cardinality({t \in commitHistory : t[1] = k}) <= 1
RecoveryScansCommitted == recoveryOK
NoRecoveredReceipt == ~recoveredReceipt
NoRecoveredDispatch == ~recoveredDispatch
NoMismatchRejection == rejected = {}
NoScopedAdmissions == ~ (\A k \in Keys : \E r \in observedReceipts : r[1] = k)
=============================================================================
