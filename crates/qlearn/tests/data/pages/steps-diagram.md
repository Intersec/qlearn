# The states of a server

```steps
{
  "title": "From follower to leader",
  "columns": ["Server", "State"],
  "steps": [
    { "label": "Before", "body": "Every server is a follower.",
      "rows": [["S1", "follower"], ["S2", "follower"]] },
    { "label": "The vote", "body": "S1 asks S2 for its vote.",
      "diagram": "sequenceDiagram\n  S1->>S2: RequestVote(term 6)\n  S2-->>S1: vote granted\n" },
    { "label": "After", "body": "S1 leads.",
      "rows": [["S1", "leader"], ["S2", "follower"]] }
  ]
}
```

A block of diagrams only needs no columns.

```steps
{
  "title": "A state machine",
  "steps": [
    { "label": "One", "body": "A follower.", "diagram": "flowchart LR\n  F[Follower]\n" },
    { "label": "Two", "body": "It times out.", "diagram": "flowchart LR\n  F[Follower] --> C[Candidate]\n" }
  ]
}
```
