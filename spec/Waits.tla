---------------------------- MODULE Waits ----------------------------
EXTENDS Naturals, FiniteSets
CONSTANTS Work, MaxAttempts, UnsafeAdmission, UnsafeRetry
VARIABLES edges, checked, attempts, identity
vars == <<edges, checked, attempts, identity>>
Pairs == Work \X Work
RECURSIVE Reach(_, _)
Reach(e, n) == IF n = 0 THEN e ELSE
    LET r == Reach(e, n - 1) IN
    r \cup {<<a, c>> \in Pairs : \E b \in Work :
                                    <<a, b>> \in r /\ <<b, c>> \in e}
Acyclic(e) == \A w \in Work : <<w, w>> \notin Reach(e, Cardinality(Work))
Init == /\ edges = {} /\ checked = {}
        /\ attempts = [p \in Pairs |-> 0]
        /\ identity = [p \in Pairs |-> p]
\* Separate preflight checks may race; safe commits revalidate atomically.
Check(p) == /\ p \notin edges /\ p \notin checked
            /\ Acyclic(edges \cup {p})
            /\ checked' = checked \cup {p}
            /\ UNCHANGED <<edges, attempts, identity>>
Commit(p) == /\ p \in checked /\ p \notin edges
             /\ (UnsafeAdmission \/ Acyclic(edges \cup {p}))
             /\ edges' = edges \cup {p}
             /\ checked' = checked \ {p}
             /\ attempts' = [attempts EXCEPT ![p] = 1]
             /\ UNCHANGED identity
Retry(p) == /\ p \in edges /\ attempts[p] < MaxAttempts
            /\ attempts' = [attempts EXCEPT ![p] = @ + 1]
            /\ identity' = [identity EXCEPT ![p] =
                  IF UnsafeRetry THEN <<p[2], p[1]>> ELSE @]
            /\ UNCHANGED <<edges, checked>>
Finish(p) == /\ p \in edges /\ edges' = edges \ {p}
             /\ UNCHANGED <<checked, attempts, identity>>
Next == \E p \in Pairs : Check(p) \/ Commit(p) \/ Retry(p) \/ Finish(p)
Spec == Init /\ [][Next]_vars
TypeOK == /\ edges \subseteq Pairs /\ checked \subseteq Pairs
          /\ attempts \in [Pairs -> 0..MaxAttempts]
          /\ identity \in [Pairs -> Pairs]
NoWaitCycle == Acyclic(edges)
RetryIdentity == \A p \in Pairs : identity[p] = p
=============================================================================
