# wsspike

Throwaway spike for `04 Spike: workspace create and destroy under a running dev server`.

Creates a git worktree at `D:\fw\wtNNN`, starts a dev server inside it holding the handles a real one holds - a recursive `fs.watch`, an open write stream to a log in the tree, an open file handle, a listening socket - then tears the workspace down and records what blocked the delete.

Every filesystem call goes through the `\\?\` extended-length form, and the read-only bit is cleared on the way down, because git marks packfiles read-only and a plain `remove_dir_all` fails on them before it ever reaches a locked file.

## Run

```
cargo run -- cycles 50    reap the job, then tear down. The proposed strategy.
cargo run -- naive 10     tear down without reaping first. The control.
cargo run -- npm 10       node_modules as a junction to origin's.
cargo run -- npmreal 10   node_modules copied in for real. The honest heavy case.
cargo run -- cwdout 5     server's cwd set OUTSIDE the workspace. The A/B.
```

`WSSPIKE_NO_BACKOFF=1` collapses the retry schedule to a single attempt, which forces the quarantine path so the last resort can be tested rather than assumed.

The spike recreates `D:\fw` from scratch on each run and leaves it behind. Delete it when done.

## Outcomes

Teardown reports one of three outcomes, and the distinction matters - an early version of this spike conflated "not quarantined" with "deleted" and reported a clean run that was not clean.

- `Deleted` - gone.
- `Quarantined` - not deletable, but renamed out of the way.
- `Stuck` - neither. Still on disk, in place.

Results are recorded on the ticket, not here.
