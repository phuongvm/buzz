## Context

- Fork `origin/main` pre-sync: `3f66bb741`.
- Upstream `upstream/main`: `003770cb7`.
- Merge candidate SHA: `d19183fab0727a908f0fafbe6487c91b612a4755`.
- Merge conflicts occurred in `crates/buzz-acp/src/pool.rs` and `crates/buzz-acp/src/queue.rs`.

## Goals / Non-Goals

**Goals:**
- Resolve merge conflicts preserving BUZZ-INV-1..5.
- Retain upstream `reply_anchor_is_trigger` for native steering while retaining fork's `fallback_thread_tags` for direct-reply routing.
- Verify sidecar compilation (`buzz-acp`, `buzz-agent`, `buzz-cli`).
- Publish to `origin/main` and stage binaries into `%LOCALAPPDATA%\Buzz\`.

**Non-Goals:**
- Automated restart of live desktop or remote relay Docker containers.

## Decisions

### Conflict Resolution in `crates/buzz-acp/src/queue.rs`
Keep both `resolve_turn_reply_anchor` + `fallback_thread_tags` (fork) and `reply_anchor_is_trigger` (upstream). They operate on different aspects of prompt ancestry and turn routing.

### Conflict Resolution in `crates/buzz-acp/src/pool.rs`
Execute both `publication_thread_tags = crate::queue::fallback_thread_tags(...)` and `prompt_routing.record_trigger_anchor(crate::queue::reply_anchor_is_trigger(...))`.

## Risks / Trade-offs

- [Direct reply regressions] → Verified `fallback_thread_tags` logic is unchanged and compiles cleanly.
- [Sidecar compilation] → `cargo build --release -p buzz-acp -p buzz-agent -p buzz-cli` builds with 0 errors.
