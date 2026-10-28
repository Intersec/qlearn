# The election timer

The timeout has a random part:

```excerpt
path: src/raft.c
lines: 6-17
mark: 13, 15-16
hide: 13
```

Two followers rarely time out at once.
