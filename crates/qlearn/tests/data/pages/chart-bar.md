# Election time

A wider range of timeouts ends an election sooner.

```chart
{
  "kind": "bar",
  "title": "Election time, by timeout range",
  "x": { "label": "Timeout range (ms)", "values": ["150-300", "150-200", "150-155", "150-151"] },
  "y": { "label": "Median election time", "unit": "ms" },
  "series": [
    { "name": "5 servers", "values": [283, 410, 1100, 1520] },
    { "name": "9 servers", "values": [301, 455, 1290, 1985.5] }
  ]
}
```

The narrowest range splits the votes most often.
