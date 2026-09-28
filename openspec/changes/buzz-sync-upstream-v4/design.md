## Context

- Fork `origin/main` `ee13f2fd5`; upstream `upstream/main` `ebe99a46e`; merge-base `ef2aa1ae3`.
- Divergence: origin +17 (7 non-merge customizations), upstream +50.
- `git merge-tree --write-tree origin/main upstream/main` → 1 content conflict
  (`crates/buzz-acp/src/lib.rs`), 0 modify/delete. Auto-merged: `acp.rs`,
  `config.rs`, `pool.rs`, `desktop/src-tauri/src/managed_agents/personas.rs`.
- Production relay (intel_nuc `buzz-relay`, :3003) runs `a478b2f5c`, which is
  not on `origin/main` nor upstream.

## Goals / Non-Goals

**Goals:** fast-forwardable `main` containing upstream + all fork invariants +
the relay hotfix; verified by build, tests, and a shadow relay.

**Non-Goals:** rebuilding/restarting the production container (separate
Commander-approved step); upstreaming fork patches.

## Decisions

### Conflict resolution matrix

| File | Upstream | Origin | Resolution |
|------|----------|--------|------------|
| `crates/buzz-acp/src/lib.rs` (~2944) | `shutdown_tx` + signal handler hoisted before `run_harness`; old site only sends `startup_ready` | Creates `shutdown_tx`+signals at old site, spawns outbox flush task | Take upstream structure; keep outbox task, subscribing to the hoisted `shutdown_tx` (INV-ACP-01). Drop origin's duplicate channel + signal handler. |

### Hotfix re-application

Cherry-pick `fc157e9c1` (original of `a478b2f5c`) after the merge commit.
Expected overlap with #7298 in `side_effects.rs` only at the
`classify_remove_other` call site (upstream line ~543); resolve by keeping
upstream surroundings and inserting the relay-role lookup.

### Pending whitelist change

Applied from the live checkout's working-tree diff as its own signed commit.

## Risks / Trade-offs

- [Silent shadowed `shutdown_tx`] → grep for a single `broadcast::channel` of
  shutdown in `lib.rs`; cargo test buzz-acp.
- [Migration 0047 on shared Postgres] → additive; shadow relay uses a scratch
  database, never `buzz_db`.
- [Hotfix/#7298 semantic drift] → relay unit tests in `channel_authz` +
  `cargo test -p buzz-relay`.

## Migration Plan / Rollback

1. Push `sync/upstream-2026-09-28` → `origin/main` (fast-forward only) after approval.
2. Production rebuild is a separate step; rollback = retag prior image
   `buzz-buzz-relay` and `docker compose up -d buzz-relay`.
3. Git rollback: `origin/main` pre-sync SHA `ee13f2fd5` is recorded here.
