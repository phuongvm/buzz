## Why

The PTDev fork (`origin/main` = `ee13f2fd5`) has fallen 50 commits behind
`upstream/main` (`ebe99a46e`, block/buzz, 2026-09-28) since merge-base
`ef2aa1ae3` (2026-09-19). Upstream shipped relay moderation fixes (#7298 kick
side effects at convergence, migration 0047), buzz-acp startup/shutdown
refactor, and desktop fixes. The production relay on `buzz.ptdev.vip` runs a
hotfix branch (`fix/relay-owner-remove-member-on-2ca9828`, `a478b2f5c`) that is
NOT on `origin/main`; rebuilding from `origin/main` today would silently drop
that fix. This change folds upstream, the hotfix, and one pending local change
into a single fast-forwardable `main`.

## What Changes

- Merge `upstream/main` into `origin/main` on branch `sync/upstream-2026-09-28`.
- Resolve the single content conflict in `crates/buzz-acp/src/lib.rs`
  (upstream moved `shutdown_tx` / signal handling ahead of `run_harness`; the
  fork inserted the auto-publish outbox flush task at the old location).
- Re-apply `fc157e9c1` / `a478b2f5c` (relay owner/admin may remove any channel
  or DM member) on top of upstream `ce41be9c4` (#7298).
- Commit the pending managed-node command whitelist in
  `desktop/src-tauri/src/managed_agents/managed_node_paths.rs`.
- Introduce `openspec/` into the repo to host this contract.
- No breaking API changes. Upstream migration `0047_relay_admin_action_target.sql`
  is applied automatically on relay startup (additive).

## Capabilities

### New Capabilities

- `fork-customizations`: Behavioral invariants of PTDev-local patches that must
  survive every upstream sync (buzz-acp auto-publish/outbox, direct replies,
  credential forwarding, provider-prefixed model switch, permission-mode wire
  mapping, desktop BOM trim, relay owner/admin member removal, managed-node
  command whitelist).

### Modified Capabilities

- None (no prior specs exist in this repo).

## Impact

- Code: `crates/buzz-acp/*`, `crates/buzz-relay/src/handlers/{channel_authz,side_effects}.rs`,
  `desktop/src/features/channels/ui/MembersSidebar.tsx`,
  `desktop/src-tauri/src/managed_agents/managed_node_paths.rs`, plus all upstream files.
- Database: migration 0047 (additive column/table for admin action target).
- Runtime: `buzz-relay` container on intel_nuc (`:3003`) is rebuilt only after
  shadow verification on an alternate port and Commander approval.
- Rollback: previous image `buzz-buzz-relay` retained; `origin/main` can be
  reset by the Commander to `ee13f2fd5` (pre-sync) if required.
