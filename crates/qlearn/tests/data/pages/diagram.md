# The states of a server

A server is a follower, a candidate, or the leader.

```mermaid
stateDiagram-v2
  [*] --> Follower
  Follower --> Candidate: times out
  Candidate --> Leader: wins the vote
  Candidate --> Follower: hears a leader
  Leader --> Follower: hears a higher term
```

The text after a diagram opens a new block. The source of a diagram is
kept as the agent typed it, `<` and `&` included: Mermaid escapes it.

```mermaid
graph LR
  A["term < 6 & no vote"] --> B[votes]
```
