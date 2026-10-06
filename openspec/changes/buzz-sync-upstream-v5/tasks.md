## 1. Reconnaissance

- [x] 1.1 Probed upstream divergence against `upstream/main` (10 commits ahead)
- [x] 1.2 Isolated worktree created at `O:\workspaces\.worktrees\wt-buzz-sync-daily`

## 2. OpenSpec Contract

- [x] 2.1 Scaffold `buzz-sync-upstream-v5` change specification
- [x] 2.2 Validate strict syntax with `openspec validate buzz-sync-upstream-v5 --strict`

## 3. Active Reconciliation in Worktree

- [x] 3.1 Reconcile merge conflicts in `crates/buzz-acp/src/pool.rs`
- [x] 3.2 Reconcile merge conflicts in `crates/buzz-acp/src/queue.rs`
- [x] 3.3 Verify Invariants BUZZ-INV-1 through BUZZ-INV-5

## 4. Verification & Packaging

- [x] 4.1 Compile release sidecars (`buzz-acp`, `buzz-agent`, `buzz-cli`)
- [x] 4.2 Verify candidate commit `d19183fab0727a908f0fafbe6487c91b612a4755`

## 5. Publication & Live Cutover

- [x] 5.1 Push verified branch to `origin/main`
- [x] 5.2 Fast-forward live repo `O:\workspaces\oss\buzz`
- [x] 5.3 Stage sidecar binaries into `%LOCALAPPDATA%\Buzz\`
