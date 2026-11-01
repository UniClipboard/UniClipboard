--------------------------- MODULE ReceiveReply ---------------------------
EXTENDS Naturals
CONSTANTS MaxGeneration, MaxReads, FenceReplies
VARIABLES generation, ticket, cancelled, phase, reads, staleCommitted
vars == <<generation, ticket, cancelled, phase, reads, staleCommitted>>
Init == /\ generation = 0 /\ ticket = 0 /\ cancelled = FALSE
        /\ phase = "idle" /\ reads = 0 /\ staleCommitted = FALSE
Current == ~cancelled /\ ticket = generation
StartRead == /\ phase = "idle" /\ ~cancelled /\ reads < MaxReads
             /\ ticket' = generation /\ reads' = reads + 1
             /\ phase' = "receive"
             /\ UNCHANGED <<generation, cancelled, staleCommitted>>
(* Receive replies either publish synchronously, or enter the metadata branch. *)
ReceiveReply == /\ phase = "receive"
                /\ IF FenceReplies /\ ~Current
                      THEN /\ phase' = "idle" /\ UNCHANGED staleCommitted
                      ELSE \/ /\ phase' = "idle"
                              /\ staleCommitted' = (staleCommitted \/ ~Current)
                           \/ /\ phase' = "detail" /\ UNCHANGED staleCommitted
                /\ UNCHANGED <<generation, ticket, cancelled, reads>>
(* There is no generation check between the two metadata awaits in the code. *)
DetailReply == /\ phase = "detail" /\ phase' = "confirm"
               /\ UNCHANGED <<generation, ticket, cancelled, reads, staleCommitted>>
ConfirmReply == /\ phase = "confirm" /\ phase' = "idle"
                /\ staleCommitted' = (staleCommitted \/ (~Current /\ ~FenceReplies))
                /\ UNCHANGED <<generation, ticket, cancelled, reads>>
Retire == /\ ~cancelled /\ generation < MaxGeneration
          /\ generation' = generation + 1
          /\ UNCHANGED <<ticket, cancelled, phase, reads, staleCommitted>>
Unmount == /\ ~cancelled /\ generation < MaxGeneration
           /\ cancelled' = TRUE /\ generation' = generation + 1
           /\ UNCHANGED <<ticket, phase, reads, staleCommitted>>
Done == /\ phase = "idle" /\ (cancelled \/ reads = MaxReads)
        /\ UNCHANGED vars
Next == StartRead \/ ReceiveReply \/ DetailReply \/ ConfirmReply \/ Retire \/ Unmount \/ Done
TypeOK == /\ generation \in 0..MaxGeneration /\ ticket \in 0..MaxGeneration
          /\ cancelled \in BOOLEAN /\ staleCommitted \in BOOLEAN
          /\ phase \in {"idle", "receive", "detail", "confirm"} /\ reads \in 0..MaxReads
NoRetiredCommit == ~staleCommitted
Spec == Init /\ [][Next]_vars
=============================================================================
