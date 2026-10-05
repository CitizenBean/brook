----------------------------- MODULE Retention -----------------------------
EXTENDS Naturals, FiniteSets
CONSTANTS Mutation, DiskLimit, RamLimit, UnusedTTL, RetryWindow, WorkTTL, MaxTime
Operations == {"a", "b", "c"}
Issued(k) == IF k = "c" THEN 3 ELSE 0
BodyUnits == 2
ControlUnits == 1
SnapshotUnits == 1
VARIABLES now, bodies, control, pending, terminal, queue, snapshot, created,
          terminalAt, terminalHistory, admissionCount, windowOK, blocked,
          staleRejected, controlProgress
vars == <<now, bodies, control, pending, terminal, queue, snapshot, created,
          terminalAt, terminalHistory, admissionCount, windowOK, blocked,
          staleRejected, controlProgress>>
DiskUsed == BodyUnits * Cardinality(bodies) + ControlUnits * Cardinality(control)
            + IF snapshot THEN SnapshotUnits ELSE 0
RamUsed == Cardinality(queue)
Fresh(k) == Issued(k) <= now /\ now < Issued(k) + RetryWindow
Unused(k) == k \notin bodies \cup control
Room == DiskUsed + BodyUnits + ControlUnits <= DiskLimit /\ RamUsed < RamLimit
Init ==
    /\ now = 0 /\ bodies = {} /\ control = {} /\ pending = {} /\ terminal = {}
    /\ queue = {} /\ snapshot = TRUE
    /\ created = [k \in Operations |-> 0] /\ terminalAt = [k \in Operations |-> 0]
    /\ terminalHistory = {} /\ admissionCount = [k \in Operations |-> 0]
    /\ windowOK = TRUE /\ blocked = {} /\ staleRejected = {} /\ controlProgress = FALSE
Admit(k) ==
    /\ Unused(k) /\ Issued(k) <= now
    /\ (Mutation = "forgetWindow" \/ now < Issued(k) + RetryWindow)
    /\ (Mutation = "overDisk" \/ DiskUsed + BodyUnits + ControlUnits <= DiskLimit)
    /\ (Mutation = "overRam" \/ RamUsed < RamLimit)
    /\ bodies' = bodies \cup {k} /\ control' = control \cup {k}
    /\ pending' = pending \cup {k} /\ queue' = queue \cup {k}
    /\ created' = [created EXCEPT ![k] = now]
    /\ admissionCount' = [admissionCount EXCEPT ![k] = @ + 1]
    /\ windowOK' = (windowOK /\ Fresh(k))
    /\ UNCHANGED <<now, terminal, snapshot, terminalAt, terminalHistory,
         blocked, staleRejected, controlProgress>>
Dequeue(k) ==
    /\ k \in queue /\ queue' = queue \ {k}
    /\ UNCHANGED <<now, bodies, control, pending, terminal, snapshot, created,
         terminalAt, terminalHistory, admissionCount, windowOK, blocked,
         staleRejected, controlProgress>>
Finish(k) ==
    /\ k \in pending
    /\ pending' = pending \ {k} /\ terminal' = terminal \cup {k}
    /\ queue' = queue \ {k}
    /\ terminalAt' = [terminalAt EXCEPT ![k] = now]
    /\ terminalHistory' = terminalHistory \cup {k}
    /\ controlProgress' = (controlProgress \/ DiskUsed = DiskLimit)
    \* The terminal marker consumes the already-reserved control unit.
    /\ UNCHANGED <<now, bodies, control, snapshot, created, admissionCount,
         windowOK, blocked, staleRejected>>
Expire(k) ==
    /\ k \in pending /\ now >= created[k] + WorkTTL
    /\ IF Mutation = "dropExpired" THEN
          /\ pending' = pending \ {k} /\ queue' = queue \ {k}
          /\ bodies' = bodies \ {k} /\ control' = control \ {k}
          /\ UNCHANGED <<now, terminal, snapshot, created, terminalAt,
               terminalHistory, admissionCount, windowOK, blocked, staleRejected,
               controlProgress>>
       ELSE Finish(k)
CollectBody(k) ==
    /\ k \in bodies /\ now >= created[k] + UnusedTTL
    /\ (Mutation = "collectPinned" \/ (k \notin pending /\ k \in terminal))
    /\ bodies' = bodies \ {k}
    /\ UNCHANGED <<now, control, pending, terminal, queue, snapshot, created,
         terminalAt, terminalHistory, admissionCount, windowOK, blocked,
         staleRejected, controlProgress>>
CollectMarker(k) ==
    /\ k \in terminal /\ k \in control /\ k \notin bodies
    /\ now >= terminalAt[k] + UnusedTTL /\ now >= Issued(k) + RetryWindow
    /\ control' = control \ {k} /\ terminal' = terminal \ {k}
    /\ UNCHANGED <<now, bodies, pending, queue, snapshot, created, terminalAt,
         terminalHistory, admissionCount, windowOK, blocked, staleRejected,
         controlProgress>>
CollectSnapshot ==
    /\ Mutation = "collectSnapshot" /\ snapshot /\ now >= UnusedTTL
    /\ snapshot' = FALSE
    /\ UNCHANGED <<now, bodies, control, pending, terminal, queue, created,
         terminalAt, terminalHistory, admissionCount, windowOK, blocked,
         staleRejected, controlProgress>>
Backpressure(k) ==
    /\ Unused(k) /\ Fresh(k) /\ ~Room
    /\ blocked' = blocked \cup {k}
    /\ UNCHANGED <<now, bodies, control, pending, terminal, queue, snapshot,
         created, terminalAt, terminalHistory, admissionCount, windowOK,
         staleRejected, controlProgress>>
RejectStale(k) ==
    /\ now >= Issued(k) + RetryWindow
    /\ staleRejected' = staleRejected \cup {k}
    /\ UNCHANGED <<now, bodies, control, pending, terminal, queue, snapshot,
         created, terminalAt, terminalHistory, admissionCount, windowOK,
         blocked, controlProgress>>
Tick == /\ now < MaxTime /\ now' = now + 1
        /\ UNCHANGED <<bodies, control, pending, terminal, queue, snapshot,
             created, terminalAt, terminalHistory, admissionCount, windowOK,
             blocked, staleRejected, controlProgress>>
Next == Tick \/ CollectSnapshot \/ (\E k \in Operations : Admit(k) \/ Dequeue(k)
     \/ Finish(k) \/ Expire(k) \/ CollectBody(k) \/ CollectMarker(k)
     \/ Backpressure(k) \/ RejectStale(k))
Spec == Init /\ [][Next]_vars
TypeOK ==
    /\ now \in 0..MaxTime /\ bodies \subseteq Operations /\ control \subseteq Operations
    /\ pending \subseteq Operations /\ terminal \subseteq Operations /\ queue \subseteq Operations
    /\ snapshot \in BOOLEAN /\ created \in [Operations -> 0..MaxTime]
    /\ terminalAt \in [Operations -> 0..MaxTime] /\ terminalHistory \subseteq Operations
    /\ admissionCount \in [Operations -> Nat] /\ windowOK \in BOOLEAN
    /\ blocked \subseteq Operations /\ staleRejected \subseteq Operations
    /\ controlProgress \in BOOLEAN
BoundedDisk == DiskUsed <= DiskLimit
BoundedRam == RamUsed <= RamLimit
LivePinsRecoverable == pending \subseteq bodies \cap control
CurrentSnapshotRetained == snapshot
TerminalBeforeReclaim == \A k \in Operations : admissionCount[k] > 0 =>
    k \in pending \cup terminalHistory
RetryWindowEnforced == windowOK
NoReadmission == \A k \in Operations : admissionCount[k] <= 1
NoControlProgress == ~controlProgress
NoBackpressure == blocked = {}
NoCleanupAfterUnpin == ~ (\E k \in terminalHistory : k \notin bodies
    /\ terminalAt[k] >= created[k] + UnusedTTL)
NoStaleRetryRejection == ~ (\E k \in staleRejected \cap terminalHistory : k \notin control)
=============================================================================
