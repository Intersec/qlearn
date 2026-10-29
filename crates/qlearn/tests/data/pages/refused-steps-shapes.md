# Shapes

```steps
["t = 0", "t = 1"]
```

```steps
{
  "title": 3,
  "columns": "Server",
  "steps": { "label": "t = 0" }
}
```

```steps
{
  "title": "Votes",
  "columns": ["Server", 2, null],
  "steps": [
    "t = 0",
    { "label": 1, "body": ["S1"], "rows": "S1" },
    { "label": "t = 2", "body": "S1 asks.", "rows": ["S1", ["S2", true, null]] },
    { "label": "t = 3", "body": "S1 leads.", "diagram": 3 }
  ]
}
```
