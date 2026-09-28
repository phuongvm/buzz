## 1. Reconnaissance

- [x] 1.1 Record merge-base `ef2aa1ae3` and divergence (origin +17, upstream +50)
- [x] 1.2 `git merge-tree --write-tree` simulation: 1 content conflict, 0 modify/delete

## 2. Contract

- [x] 2.1 Scaffold `buzz-sync-upstream-v4` with proposal, spec, design, tasks
- [ ] 2.2 `openspec validate buzz-sync-upstream-v4 --strict` passes

## 3. Merge in isolated worktree

- [x] 3.1 Worktree `O:/workspaces/wt-buzz-sync` on `sync/upstream-2026-09-28` from `origin/main`
- [ ] 3.2 `git merge --no-commit upstream/main`; resolve `lib.rs` per INV-ACP-01
- [ ] 3.3 Commit merge (signed-off)
- [ ] 3.4 Cherry-pick `fc157e9c1` (INV-RELAY-01), resolve against #7298
- [ ] 3.5 Commit managed-node whitelist (INV-DESK-02)
- [ ] 3.6 `cargo check` workspace + `desktop/src-tauri`

## 4. Verification

- [ ] 4.1 `cargo fmt --check`, `cargo clippy` (workspace + tauri)
- [ ] 4.2 `cargo test -p buzz-acp -p buzz-relay --lib`; tauri managed_agents + util tests
- [ ] 4.3 Build relay image on intel_nuc from the sync branch
- [ ] 4.4 Run shadow relay on alternate port with scratch DB; probe NIP-11 + readiness

## 5. Publication

- [ ] 5.1 Tri-Part report to Commander
- [ ] 5.2 On approval: `git push origin sync/upstream-2026-09-28:main` (fast-forward)
- [ ] 5.3 Live checkout: `git fetch origin` only
