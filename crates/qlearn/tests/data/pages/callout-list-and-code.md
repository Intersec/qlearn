> [!pitfall]
> Two calls of `grant_vote()` in one term:
>
> - the first one sets `s->voted_for`;
> - the second one returns **false**, unless the same candidate asks.
>
> ```c
> grant_vote(s, 6, 1);
> ```
>
> > A plain quote inside a callout stays a quote.
