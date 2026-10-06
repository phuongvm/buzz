## Why

The fork (`origin/main` = `3f66bb741`) fell 10 commits behind `upstream/main` (`003770cb7`, block/buzz). Upstream shipped agent management updates (Claude effort picker, session config capture, managed agent storage/nest/persona updates), personal read state (migration 0056, db store, buzz_v1 api endpoints), push gateway updates, and acp queue enhancements (`reply_anchor_is_trigger` for native steer tracking). This sync reconciles fork invariants BUZZ-INV-1..5 with upstream, verifies release sidecars, and publishes the fast-forwardable merge to `origin/main`.

## What Changes

- Merge `upstream/main` (`003770cb7`) into `origin/main` producing candidate `d19183fab`.
- Reconcile merge conflicts in `crates/buzz-acp/src/pool.rs` and `crates/buzz-acp/src/queue.rs`:
  - Retain upstream `reply_anchor_is_trigger` to support native steering guards.
  - Retain fork `fallback_thread_tags` and `resolve_turn_reply_anchor` to enforce `BUZZ_REPLY_IN_THREAD=false` direct-reply routing.
- Reconcile `run_prompt_task` in `pool.rs` to invoke both `publication_thread_tags` fallback and `prompt_routing.record_trigger_anchor`.
- Preserve Invariants BUZZ-INV-1 through BUZZ-INV-5.
- Verify release sidecar compilation (`buzz-acp`, `buzz-agent`, `buzz-cli`).
- Stage compiled binaries into `%LOCALAPPDATA%\Buzz\`.

## Capabilities

### Modified Capabilities

- `fork-customizations`: Behavioral invariants of fork patches surviving upstream syncs (direct replies, auto-publish outbox, credential forwarding, provider prefixing, managed node whitelist, relay authorization, websocket keepalive).

## Impact

- Code: `crates/buzz-acp/src/pool.rs`, `crates/buzz-acp/src/queue.rs`, plus upstream desktop agent dialogs, personal read state in `buzz-db` and `buzz-relay`.
- Database: Upstream migration `0056_personal_read_state.sql` (additive).
- Runtime: Release sidecars staged offline in `%LOCALAPPDATA%\Buzz\`. Desktop UI and remote NUC relay Docker containers remain stable under Commander manual control.
