---------------------------- MODULE Ownership ----------------------------
EXTENDS Naturals, FiniteSets
CONSTANTS Mutation, MaxTime, LeaseDuration, MaxGeneration
Sessions == {"sA", "sB"}
Workers == {"w1", "w2"}
Other(s) == IF s = "sA" THEN "sB" ELSE "sA"
Tokens == Sessions \X Workers \X (1..MaxGeneration) \X (1..MaxGeneration)
VARIABLES now, state, issued, preflight, commitOK, heartbeatOK, claimOK,
          localityOK, transferred, transferResumed, reacquired, renewed
vars == <<now, state, issued, preflight, commitOK, heartbeatOK, claimOK,
          localityOK, transferred, transferResumed, reacquired, renewed>>
Init ==
    /\ now = 0
    /\ state = [s \in Sessions |->
          [owner |-> "none", generation |-> 0, serial |-> 0, expiry |-> 0,
           cancelGeneration |-> 0, pending |-> TRUE, done |-> FALSE]]
    /\ issued = {} /\ preflight = {} /\ transferred = {}
    /\ commitOK = TRUE /\ heartbeatOK = TRUE /\ claimOK = TRUE /\ localityOK = TRUE
    /\ transferResumed = FALSE /\ reacquired = FALSE /\ renewed = FALSE
Eligible(s) == state[s].owner = "none" \/ now >= state[s].expiry
Current(t) == state[t[1]].owner = t[2] /\ state[t[1]].generation = t[3]
Live(t) == Current(t) /\ now < state[t[1]].expiry
\* Serial is an observation of grant incarnation, not another worker credential.
Valid(t) == Live(t) /\ state[t[1]].serial = t[4]
CanWrite(t) == now < state[t[1]].expiry /\
              (Mutation = "staleCommit" \/ Current(t))
GrantRecord(s, w) ==
    [owner |-> w,
     generation |-> IF Mutation = "reuseGeneration" /\ state[s].generation > 0
                    THEN state[s].generation ELSE state[s].generation + 1,
     serial |-> state[s].serial + 1,
     expiry |-> now + LeaseDuration,
     cancelGeneration |-> IF Mutation = "takeoverCancels" THEN 1
                          ELSE state[s].cancelGeneration,
     pending |-> IF Mutation = "takeoverCancels" THEN FALSE ELSE state[s].pending,
     done |-> state[s].done]
Grant(s, w, expected) ==
    LET next == GrantRecord(s, w)
        token == <<s, w, next.generation, next.serial>>
    IN /\ state[s].serial < MaxGeneration
       /\ (Mutation \in {"stealLive", "racyClaim"} \/ Eligible(s))
       /\ (Mutation = "racyClaim" \/ expected = state[s].serial)
       /\ state' = [t \in Sessions |->
            IF t = s THEN next
            ELSE IF Mutation = "globalClaim" THEN [state[t] EXCEPT !.owner = w]
            ELSE state[t]]
       /\ issued' = issued \cup {token}
       /\ preflight' = IF Mutation = "racyClaim"
                        THEN preflight \ {<<s, w, expected>>} ELSE preflight
       /\ claimOK' = (claimOK /\ Eligible(s) /\ expected = state[s].serial)
       /\ localityOK' = (localityOK /\ state'[Other(s)] = state[Other(s)])
       /\ transferred' = IF state[s].owner \in Workers /\ state[s].owner # w
                              /\ state[s].pending
                          THEN transferred \cup {token} ELSE transferred
       /\ reacquired' = (reacquired \/
            (state[s].owner = "none" /\ \E old \in issued :
                old[1] = s /\ old[2] = w /\ old[3] < next.generation))
       /\ UNCHANGED <<now, commitOK, heartbeatOK, transferResumed, renewed>>
Claim(s, w) == /\ Mutation # "racyClaim" /\ Grant(s, w, state[s].serial)
CheckClaim(s, w) ==
    /\ Mutation = "racyClaim" /\ Eligible(s) /\ state[s].serial < MaxGeneration
    /\ preflight' = preflight \cup {<<s, w, state[s].serial>>}
    /\ UNCHANGED <<now, state, issued, commitOK, heartbeatOK, claimOK,
         localityOK, transferred, transferResumed, reacquired, renewed>>
RacyGrant(p) == /\ Mutation = "racyClaim" /\ p \in preflight
               /\ Grant(p[1], p[2], p[3])
Heartbeat(t) ==
    /\ t \in issued
    /\ (Mutation = "staleHeartbeat" \/ Current(t))
    /\ (Mutation = "expiredRenew" \/ now < state[t[1]].expiry)
    /\ state' = [state EXCEPT ![t[1]].expiry = now + LeaseDuration]
    /\ heartbeatOK' = (heartbeatOK /\ Valid(t))
    /\ renewed' = (renewed \/ now + LeaseDuration > state[t[1]].expiry)
    /\ UNCHANGED <<now, issued, preflight, commitOK, claimOK, localityOK,
         transferred, transferResumed, reacquired>>
Release(t) ==
    /\ t \in issued /\ CanWrite(t)
    /\ state' = [state EXCEPT ![t[1]].owner = "none", ![t[1]].expiry = now]
    /\ commitOK' = (commitOK /\ Valid(t))
    /\ UNCHANGED <<now, issued, preflight, heartbeatOK, claimOK, localityOK,
         transferred, transferResumed, reacquired, renewed>>
ProtectedCommit(t) ==
    /\ t \in issued /\ CanWrite(t)
    /\ commitOK' = (commitOK /\ Valid(t))
    /\ UNCHANGED <<now, state, issued, preflight, heartbeatOK, claimOK, localityOK,
         transferred, transferResumed, reacquired, renewed>>
Resume(t) ==
    /\ t \in issued /\ CanWrite(t) /\ state[t[1]].pending
    /\ state' = [state EXCEPT ![t[1]].pending = FALSE, ![t[1]].done = TRUE]
    /\ commitOK' = (commitOK /\ Valid(t))
    /\ transferResumed' = (transferResumed \/ t \in transferred)
    /\ UNCHANGED <<now, issued, preflight, heartbeatOK, claimOK, localityOK,
         transferred, reacquired, renewed>>
Tick == /\ now < MaxTime /\ now' = now + 1
        /\ UNCHANGED <<state, issued, preflight, commitOK, heartbeatOK, claimOK,
             localityOK, transferred, transferResumed, reacquired, renewed>>
Next == Tick \/ (\E s \in Sessions, w \in Workers : Claim(s, w) \/ CheckClaim(s, w))
     \/ (\E p \in preflight : RacyGrant(p))
     \/ (\E t \in issued : Heartbeat(t) \/ Release(t) \/ ProtectedCommit(t) \/ Resume(t))
Spec == Init /\ [][Next]_vars
TypeOK ==
    /\ now \in 0..MaxTime
    /\ state \in [Sessions ->
          [owner : Workers \cup {"none"}, generation : 0..MaxGeneration,
           serial : 0..MaxGeneration, expiry : 0..(MaxTime + LeaseDuration),
           cancelGeneration : 0..1, pending : BOOLEAN, done : BOOLEAN]]
    /\ issued \subseteq Tokens /\ transferred \subseteq Tokens
    /\ preflight \subseteq (Sessions \X Workers \X (0..MaxGeneration))
    /\ commitOK \in BOOLEAN /\ heartbeatOK \in BOOLEAN /\ claimOK \in BOOLEAN
    /\ localityOK \in BOOLEAN /\ transferResumed \in BOOLEAN
    /\ reacquired \in BOOLEAN /\ renewed \in BOOLEAN
FencedWrites == commitOK
HeartbeatOnlyLive == heartbeatOK
ClaimOnlyEligible == claimOK
SessionLocalOwnership == localityOK
OwnershipPreservesWork == \A s \in Sessions :
    state[s].cancelGeneration = 0 /\ (state[s].pending \/ state[s].done)
        /\ ~(state[s].pending /\ state[s].done)
NoTransferredResume == ~transferResumed
NoReacquisition == ~reacquired
NoHeartbeatExtension == ~renewed
=============================================================================
