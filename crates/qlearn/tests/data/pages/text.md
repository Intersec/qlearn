The leader sends a heartbeat every **50 ms**, and a follower that hears
nothing for a *random* time starts an election.

- A follower waits.
- A candidate asks for votes.
  1. It votes for itself.
  2. It sends `RequestVote` to every peer.

The paper is at [the Raft site](https://raft.github.io), and the
notes of the course are in [notes](notes/raft.md).
A line ends here,  
and a hard break keeps the next one apart.

- [x] Read the paper
- [ ] Read the code

~~The old rule~~ is gone.

---

An image inside a line is not a block: ![the flow](img/flow.png) and
![a logo](https://example.com/logo.png) both stay links.

> A quote with no marker stays text.
