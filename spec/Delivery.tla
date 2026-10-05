----------------------------- MODULE Delivery -----------------------------
EXTENDS Naturals, FiniteSets
CONSTANT Mutation
Intents == {"m1", "m2"}
Endpoints == {"A-chat", "A-email", "B-chat", "B-email"}
\* Each endpoint abstracts namespace/provider/sending-account/recipient/thread.
Allowed == {"A-chat", "A-email"}
\* Both intents originate in A's chat session; email is a valid explicit choice.
VARIABLES status, target, boundVersion, boundSerial, checked,
          route, version, serial, active, attempts, admitted,
          approvalOK, admissionOK, targetOK
vars == <<status, target, boundVersion, boundSerial, checked,
          route, version, serial, active, attempts, admitted,
          approvalOK, admissionOK, targetOK>>
Init == /\ status = [i \in Intents |-> "new"]
        /\ target = [i \in Intents |-> "A-chat"]
        /\ boundVersion = [i \in Intents |-> 0]
        /\ boundSerial = [i \in Intents |-> 0]
        /\ checked = {}
        /\ route = [d \in Endpoints |-> d]
        /\ version = [d \in Endpoints |-> 0]
        /\ serial = [d \in Endpoints |-> 0]
        /\ active = Endpoints
        /\ attempts = [i \in Intents |-> 0] /\ admitted = {}
        /\ approvalOK = TRUE /\ admissionOK = TRUE /\ targetOK = TRUE
Authorize(i, d) ==
    /\ status[i] = "new" /\ d \in active
    /\ (Mutation = "unauthorized" \/ d \in Allowed)
    /\ route[d] = d
    /\ status' = [status EXCEPT ![i] = "ready"]
    /\ target' = [target EXCEPT ![i] = d]
    /\ boundVersion' = [boundVersion EXCEPT ![i] = version[d]]
    /\ boundSerial' = [boundSerial EXCEPT ![i] = serial[d]]
    /\ approvalOK' = (approvalOK /\ d \in Allowed)
    /\ UNCHANGED <<checked, route, version, serial, active, attempts,
                    admitted, admissionOK, targetOK>>
Valid(i) == target[i] \in active /\ boundVersion[i] = version[target[i]]
Check(i) == /\ status[i] = "ready" /\ Valid(i) /\ i \notin checked
            /\ checked' = checked \cup {i}
            /\ UNCHANGED <<status, target, boundVersion, boundSerial, route,
                 version, serial, active, attempts, admitted, approvalOK,
                 admissionOK, targetOK>>
Revoke(d) == /\ d \in active /\ active' = active \ {d}
             /\ UNCHANGED <<status, target, boundVersion, boundSerial, checked,
                  route, version, serial, attempts, admitted, approvalOK,
                  admissionOK, targetOK>>
Rotate(d, replacement) ==
    /\ serial[d] = 0 /\ replacement # d
    /\ serial' = [serial EXCEPT ![d] = 1]
    /\ version' = [version EXCEPT ![d] = IF Mutation = "reuseVersion" THEN 0 ELSE 1]
    /\ route' = [route EXCEPT ![d] = replacement]
    /\ UNCHANGED <<status, target, boundVersion, boundSerial, checked, active,
         attempts, admitted, approvalOK, admissionOK, targetOK>>
Admit(i, payloadTarget) ==
    LET actual == CASE Mutation = "payloadTarget" -> payloadTarget
                    [] Mutation = "lateLookup" -> route[target[i]]
                    [] OTHER -> target[i]
    IN /\ status[i] = "ready" /\ i \in checked /\ attempts[i] < 2
       /\ (Mutation \in {"staleCheck", "lateLookup"} \/ Valid(i))
       /\ attempts' = [attempts EXCEPT ![i] = @ + 1]
       /\ admitted' = admitted \cup {<<i, actual>>}
       /\ targetOK' = (targetOK /\ actual = target[i])
       /\ admissionOK' = (admissionOK /\ target[i] \in Allowed
            /\ target[i] \in active /\ boundSerial[i] = serial[target[i]])
       /\ UNCHANGED <<status, target, boundVersion, boundSerial, checked,
            route, version, serial, active, approvalOK>>
Next == (\E i \in Intents, d \in Endpoints : Authorize(i, d) \/ Admit(i, d))
     \/ (\E i \in Intents : Check(i))
     \/ (\E d \in Endpoints : Revoke(d) \/
                             (\E e \in Endpoints : Rotate(d, e)))
Spec == Init /\ [][Next]_vars
TypeOK == /\ status \in [Intents -> {"new", "ready"}]
          /\ target \in [Intents -> Endpoints]
          /\ boundVersion \in [Intents -> 0..1] /\ boundSerial \in [Intents -> 0..1]
          /\ checked \subseteq Intents /\ route \in [Endpoints -> Endpoints]
          /\ version \in [Endpoints -> 0..1] /\ serial \in [Endpoints -> 0..1]
          /\ active \subseteq Endpoints /\ attempts \in [Intents -> 0..2]
          /\ admitted \subseteq (Intents \X Endpoints)
          /\ approvalOK \in BOOLEAN /\ admissionOK \in BOOLEAN /\ targetOK \in BOOLEAN
AuthorizedIntent == approvalOK
ImmutableDestination == targetOK
AuthorizedAtAdmission == admissionOK
NoCrossChannelDelivery == ~ (\E i \in Intents : <<i, "A-email">> \in admitted)
NoTwoDestinations == ~ (\E i, j \in Intents :
    <<i, "A-email">> \in admitted /\ <<j, "A-chat">> \in admitted)
=============================================================================
