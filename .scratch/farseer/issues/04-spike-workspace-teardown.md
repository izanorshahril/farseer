# Spike: workspace create and destroy under a running dev server

Type: task
Status: closed
Blocked by: none

## Question

Throwaway spike. The single most common firstmate-on-Windows failure.

Prove:

- Create a git worktree at a short root (`D:\fw\<hash>`) with `\\?\` long paths, install dependencies, start a dev server inside it.
- Attempt teardown. Observe exactly which handles block the delete: dev server, watcher, Defender, indexer, editor.
- Implement the proposed supervised teardown state machine: stop children, wait for release, retry with backoff, then quarantine.
- Measure how often quarantine is hit over roughly 50 cycles.
- Measure the same on a Dev Drive (ReFS) volume if available, including block-clone snapshot as an alternative strategy.

The answer decides whether `worktree` or `snapshot` is the default isolation strategy.

## Carried from 03

The same pid-reuse hazard applies here.
Windows recycles pids aggressively, so any check of the form "which process still holds this file lock" must key on `(pid, creation_time)` rather than a pid alone.
The spike terminated an unrelated desktop application by getting exactly this wrong.

Also relevant: reaping a five-deep tree via job handle close completes in roughly 300 to 400 microseconds.
If a workspace still cannot be deleted after the job closes, the cause is a file handle or an antivirus scan, not a process that has not exited yet.
That distinction is most of what this spike needs to establish.

## Resolution

Resolved 2026-08-22.
Spike code: [spikes/wsspike](../spikes/wsspike/README.md).
Environment: git 2.54.0.windows.1, node v24.19.0, Windows 11 build 26200.8875, Defender real-time protection **on**, workspace root `D:\fw` on NTFS.

### Verdict

**`worktree` is the default isolation strategy.** Reap first, then delete, and there is nothing left to be clever about.

| Run | Cycles | Deleted 1st attempt | Quarantined | Stuck | p50 / p95 |
| --- | --- | --- | --- | --- | --- |
| reap, then delete | 50 | **38 (76%)** | 0 | **0** | 2.5ms / 38.5ms |
| reap, then delete, real 48MB `node_modules` | 10 | **10 (100%)** | 0 | **0** | 278ms / 283ms |
| delete without reaping | 10 | 0 | 0 | **10 (100%)** | 4.29s / 4.29s |

Zero failures in 60 supervised cycles.
The retries that did happen never needed more than three attempts, roughly 35ms of backoff, against a budget of 4.3 seconds.

### What actually blocks the delete

**Every single blocked attempt, in every mode, was `ERROR_SHARING_VIOLATION` (32) on the workspace root directory.**
Never on a file. Never on a subdirectory. Never `ACCESS_DENIED`. Never Defender.

The ticket listed five suspects - dev server, watcher, Defender, indexer, editor.
An A/B run isolated the real one.
Same live dev server, same recursive `fs.watch`, same open log stream, same open file handle, single delete attempt, no reap.
The only variable was the server process's **current working directory**:

| Server cwd | Deleted 1st attempt | Stuck |
| --- | --- | --- |
| inside the workspace | 0/5 | **5/5** |
| outside the workspace | **5/5** | 0 |

**A process's current directory is an open handle on that directory, and it is opened without `FILE_SHARE_DELETE`.**
That one handle is the entire problem.
A recursive `fs.watch` over the same tree blocked nothing.

So the mitigation is not a state machine, it is a spawn-time decision.
Where farseer controls the command line it should **set the worker's cwd to the workspace's parent and pass the workspace path as an argument**, which removes the blocker outright.
Where it does not control that - most real runners expect to run *in* the workspace - reaping the job first releases the handle in well under a millisecond, per `03`, and the measured result is 0 stuck in 60 cycles.

### Quarantine does not work, and the ticket's state machine was wrong

The proposed fallback was: retry with backoff, then **quarantine by renaming the directory out of the way**.
Forcing that path with `WSSPIKE_NO_BACKOFF=1` proved it fails.

**Rename failed 5 out of 5 times**, for the same reason the delete failed.
A directory held open as a process's cwd cannot be renamed either.
Quarantine-by-rename only helps when a *file inside* the tree is locked, which the spike never once observed.

The honest ladder is therefore two rungs, not three:

1. Reap the job. Sub-millisecond, per `03`.
2. Delete with a short backoff. 76% first attempt, 100% within three.

There is no third rung, because in the one case where step 2 fails, step 3 fails for the identical reason.
If a workspace is ever genuinely stuck, the correct behaviour is to **mark it stuck in the record and surface it to the operator**, not to invent a fallback that cannot work.
Deleting on next startup is the plausible sweep, and it belongs to whatever ticket owns startup.

### Two things that surprised

**Bigger workspaces tear down more reliably, not less.**
The light workspace needed a retry 24% of the time; the 48MB, 988-file `node_modules` workspace needed one **0%** of the time, despite taking 100x longer.
The delete walks the tree depth-first and only reaches the root directory at the very end, by which time the just-reaped process's cwd handle has long since been released.
The slow part is its own backoff.

**Naive teardown corrupts rather than fails.**
Without a reap, the delete removed files out from under a live server, and the server process **survived every time**.
That is worse than a clean failure: a running dev server pointed at a half-deleted tree, with the workspace still on disk and still registered as a git worktree.
This is the firstmate failure shape, and it argues that reaping is not an optimisation but an ordering constraint.
**The delete must not begin until the job is closed.**

### Implementation notes carried into the runtime

- **`\\?\` on every path**, and it must be fully qualified with backslashes only, since the prefix disables normalisation.
- **Clear `FILE_ATTRIBUTE_READONLY` on the way down.** git marks packfiles read-only, so a plain recursive delete dies inside `.git` before reaching anything interesting. This is a silent, guaranteed failure that has nothing to do with locking.
- **`git worktree prune` after the directory is gone**, or the worktree stays registered and the branch cannot be reused.
- Pid-based liveness from `03` applies: identity is `(pid, creation_time)`.

### Not tested, and why

**Dev Drive / ReFS and block-clone snapshots were not evaluated.**
Both volumes on this machine are NTFS, and `fsutil devdrv` requires administrator rights, which this environment does not have.
Creating a Dev Drive needs an admin-created VHD.

The consequence is bounded.
`worktree` is chosen on its own merits - 0 failures in 60 cycles, p95 under 40ms - not by beating `snapshot`.
`snapshot` remains unevaluated rather than rejected, and it would be a performance optimisation on a strategy that already works, not a fix for a strategy that does not.
It needs a Dev Drive to test, so it goes to the fog rather than staying on this ticket.

### Tickets this informs

- `05 worker control contract` - teardown ordering is a hard constraint: reap the job, then delete. A workspace that will not delete is a state the operator must see, not a state to paper over.
