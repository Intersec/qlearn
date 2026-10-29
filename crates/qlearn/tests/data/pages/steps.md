# Two candidates

S1 and S2 time out close together.

```steps
{
  "title": "Two candidates in term 6",
  "columns": ["Server", "Term", "State", "Voted for"],
  "steps": [
    { "label": "t = 0 ms", "body": "S1 times out first, and votes for *itself*.",
      "rows": [["S1", 6, "candidate", "S1"], ["S2", 5, "follower", ""]] },
    { "label": "t = 2 ms", "body": "S2 gets the `RequestVote` of S1. <b>No script</b> here.",
      "rows": [["S1", 6, "candidate", "S1"], ["S2", 6, "follower", "S1"]] },
    { "label": "t = 3 ms <S3>", "body": "S3 joins: a row more.",
      "rows": [["S1", 6, "leader", "S1"], ["S2", 6, "follower", "S1"], ["S3", 6, "follower", "S1"]] }
  ]
}
```

Stop on the second step, and answer the quiz.
