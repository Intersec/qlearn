# Terms over time

```chart
{
  "kind": "line",
  "title": "The term of each server, second by second",
  "x": { "label": "Time (s)", "values": [0, 1, 2, 3, 4, 5] },
  "y": { "label": "Term" },
  "series": [
    { "name": "S1", "values": [1, 1, 2, null, 4, 4] },
    { "name": "S2", "values": [1, 2, 2, 3, 4, -1] }
  ]
}
```

S1 was down at second 3, so the line has a gap there.
