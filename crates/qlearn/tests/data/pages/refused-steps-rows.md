# Rows

```steps
{
  "title": "Votes",
  "columns": ["Server", "Term"],
  "steps": [
    { "label": "Neither", "body": "No rows, no diagram." },
    { "label": "Null", "body": "Rows at null count as none.", "rows": null },
    { "label": "Both", "body": "Rows and a diagram.", "rows": [["S1", 6]], "diagram": "graph LR\n  A --> B\n" },
    { "label": "Short", "body": "A row of one cell.", "rows": [["S1", 6], ["S2"]] },
    { "label": "Long", "body": "A row of three cells.", "rows": [["S1", 6, "leader"]] },
    { "label": "Empty", "body": "No row at all.", "rows": [] }
  ]
}
```

```steps
{
  "title": "No head",
  "steps": [
    { "label": "t = 0", "body": "Rows with no columns.", "rows": [["S1", 6]] }
  ]
}
```

```steps
{
  "title": "An empty head",
  "columns": [],
  "steps": [
    { "label": "t = 0", "body": "Rows with no columns.", "rows": [["S1", 6]] }
  ]
}
```
