----------------------------- MODULE Routing -----------------------------
EXTENDS Naturals, FiniteSets
CONSTANT Mutation
\* Four distinct internal sessions share the same external conversation ID.
Namespaces == {"accountA", "accountB"}
Clients == {"chat", "api"}
Keys == Namespaces \X Clients \X {"42"}
Sessions == {"A-chat", "A-api", "B-chat", "B-api"}
Resolve(n, c) == CASE n = "accountA" /\ c = "chat" -> "A-chat"
                  [] n = "accountA" /\ c = "api" -> "A-api"
                  [] n = "accountB" /\ c = "chat" -> "B-chat"
                  [] OTHER -> "B-api"
Registry == [k \in Keys |-> Resolve(k[1], k[2])]
Namespace(s) == IF s \in {"A-chat", "A-api"} THEN "accountA" ELSE "accountB"
Requests == {"qA", "qB"}
Source(q) == IF q = "qA" THEN "A-chat" ELSE "B-chat"
Destination(q) == IF q = "qA" THEN "A-api" ELSE "B-api"
\* Peer identity is an authenticated adapter assertion, not a payload claim.
ExpectedPeer(q) == Destination(q)
VARIABLES routed, returned, routedOK, returnOK, peerOK, spoofIgnored
vars == <<routed, returned, routedOK, returnOK, peerOK, spoofIgnored>>
Init == /\ routed = {} /\ returned = {}
        /\ routedOK = TRUE /\ returnOK = TRUE /\ peerOK = TRUE /\ spoofIgnored = FALSE
Receive(authNamespace, authClient, claimedSession) ==
    LET expected == Registry[<<authNamespace, authClient, "42">>]
        actual == CASE Mutation = "payloadRoute" -> claimedSession
                    [] Mutation = "omitNamespace" -> Resolve("accountA", authClient)
                    [] Mutation = "omitClient" -> Resolve(authNamespace, "chat")
                    [] OTHER -> expected
    IN /\ routed' = routed \cup {actual}
       /\ routedOK' = (routedOK /\ actual = expected)
       /\ spoofIgnored' = (spoofIgnored \/
            (claimedSession # expected /\ actual = expected))
       /\ UNCHANGED <<returned, returnOK, peerOK>>
Reply(q, authPeer, claimedSession) ==
    LET actual == CASE Mutation = "payloadReturn" -> claimedSession
                    [] Mutation = "destinationReturn" -> Destination(q)
                    [] OTHER -> Source(q)
    IN /\ (Mutation = "wrongPeer" \/ authPeer = ExpectedPeer(q))
       /\ returned' = returned \cup {<<q, actual>>}
       /\ returnOK' = (returnOK /\ actual = Source(q))
       /\ peerOK' = (peerOK /\ authPeer = ExpectedPeer(q))
       /\ UNCHANGED <<routed, routedOK, spoofIgnored>>
Next == (\E n \in Namespaces, c \in Clients, s \in Sessions : Receive(n, c, s))
     \/ (\E q \in Requests, p \in Sessions, s \in Sessions : Reply(q, p, s))
Spec == Init /\ [][Next]_vars
TypeOK == /\ routed \subseteq Sessions /\ returned \subseteq (Requests \X Sessions)
          /\ routedOK \in BOOLEAN /\ returnOK \in BOOLEAN /\ peerOK \in BOOLEAN /\ spoofIgnored \in BOOLEAN
RegistryIsInjective == \A a, b \in Keys : Registry[a] = Registry[b] => a = b
NoIgnoredSpoof == ~spoofIgnored
TrustedIngress == routedOK
ReturnToOrigin == returnOK
AuthenticatedReply == peerOK
=============================================================================
