# Values

```chart
{
  "kind": "bar",
  "title": "Votes per term",
  "x": { "label": "Term", "values": ["1", "2", "3", "4"] },
  "y": { "label": "Votes" },
  "series": [
    { "name": "S1", "values": [3, 2, 1] },
    { "name": "S2", "values": [3, null, "2", true] },
    { "name": "S3", "values": [3, 2, 1, 0, 5] }
  ]
}
```

```chart
{
  "kind": "line",
  "title": "Votes per term",
  "x": { "label": "Term", "values": ["1", "2"] },
  "y": { "label": "Votes" },
  "series": [
    { "name": "S1", "values": [null, null] },
    { "name": "S2", "values": [1, null] }
  ]
}
```
