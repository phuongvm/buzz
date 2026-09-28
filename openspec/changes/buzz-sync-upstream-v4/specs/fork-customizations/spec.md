## ADDED Requirements

### Requirement: INV-ACP-01 Single shutdown channel across startup and outbox

`crates/buzz-acp/src/lib.rs` SHALL create exactly one `shutdown_tx`
broadcast sender (upstream location, before `run_harness`). The auto-publish
outbox flush task MUST subscribe to that same sender and MUST NOT construct a
second `broadcast::channel` that shadows it.

#### Scenario: Signal during startup reaches the main loop

- **GIVEN** the harness is starting and the outbox task has been spawned
- **WHEN** SIGINT/SIGTERM/ctrl_c fires
- **THEN** the main event loop and the outbox task both observe shutdown via
  the single `shutdown_tx`, and the process exits without hanging

#### Scenario: Readiness signal preserved

- **WHEN** the harness completes startup
- **THEN** `startup_ready.send(())` (upstream) is still emitted exactly once

### Requirement: INV-ACP-02 Auto-publish outbox durability

The harness SHALL persist undelivered fallback events when `BUZZ_ACP_OUTPUT_MODE`
enables auto-publish: they are written to the private outbox directory, retried periodically
(30 s cadence), drained on shutdown, and deleted only after relay acceptance
(`auto_publish_outbox.rs`).

#### Scenario: Relay rejects then accepts

- **GIVEN** an auto-publish event that failed delivery and was persisted
- **WHEN** the next flush succeeds
- **THEN** the event is accepted by the relay and the slot file is removed

#### Scenario: Tool-only turns publish nothing

- **WHEN** a turn contains only tool activity with no conversational envelope
- **THEN** no fallback message is published (`tool_only_is_default_and_captures_nothing`)

### Requirement: INV-ACP-03 Direct reply enforcement

When `BUZZ_REPLY_IN_THREAD=false`, the harness SHALL instruct the agent to reply
directly in the channel and SHALL compute fallback thread tags that do not
thread the reply (`queue.rs::direct_reply_enforced`, `fallback_thread_tags`).

#### Scenario: Direct reply configured

- **GIVEN** `BUZZ_REPLY_IN_THREAD=false`
- **WHEN** a channel message triggers a turn
- **THEN** the prompt includes the direct-reply instruction and fallback tags
  carry no thread root

### Requirement: INV-ACP-04 Credential forwarding to agent subprocess

`config.rs` SHALL forward `BUZZ_PRIVATE_KEY` (hex, authoritative identity),
`BUZZ_RELAY_URL`, and when set `BUZZ_AUTH_TAG`, `BUZZ_REPLY_IN_THREAD`,
`BUZZ_REPLY_TO_MODE` into `persona_env_vars`.

#### Scenario: nsec input forwarded as hex

- **WHEN** buzz-acp is started with an `nsec` private key
- **THEN** exactly one `BUZZ_PRIVATE_KEY` entry is forwarded, in hex, with the
  same public key (`forwarded_credentials_preserve_authoritative_identity_and_relay`)

### Requirement: INV-ACP-05 Provider-prefixed model switch and permission wire mapping

`acp.rs` SHALL match a requested model id against provider-prefixed agent model
ids (e.g. `openai/<id>`), and `pool.rs` SHALL map `bypassPermissions` to
`agent-full-access` when the agent only advertises the latter.

#### Scenario: Prefixed model resolves

- **WHEN** the requested model is `cb-gemini-flash-high` and the agent lists `openai/cb-gemini-flash-high`
- **THEN** the switch resolves to the prefixed id (`resolve_matches_provider_prefixed_model`)

#### Scenario: Codex full access

- **WHEN** mode is `bypassPermissions` and the agent supports only `agent-full-access`
- **THEN** `agent-full-access` is sent (`resolve_permission_mode_wire_codex_full_access`)

### Requirement: INV-DESK-01 JSON stores tolerate UTF-8 BOM

Desktop JSON store readers SHALL strip a leading UTF-8 BOM before parsing
(`desktop/src-tauri/src/util.rs`).

#### Scenario: BOM-prefixed store

- **WHEN** a JSON store file begins with `EF BB BF`
- **THEN** it parses identically to the BOM-less file

### Requirement: INV-RELAY-01 Relay owner/admin may remove any channel or DM member

`channel_authz::classify_remove_other` SHALL accept the actor's relay role and
return `Allow` when that role is `owner` or `admin`, regardless of channel
membership. `side_effects.rs` SHALL look up the relay role via
`get_relay_member` before classifying kind:9001 remove-other. The desktop
`MembersSidebar` SHALL offer removal when `canModerate` is true and the target
is not the current user.

#### Scenario: Relay owner removes stray DM participant

- **GIVEN** a DM where every participant has channel role `member`
- **WHEN** the relay owner publishes kind:9001 removing another participant
- **THEN** the relay accepts the event

#### Scenario: Plain relay member gains nothing

- **WHEN** an actor with relay role `member` tries to remove a non-owned member
- **THEN** the decision is `CheckAgentOwner` (or `Deny` if not in the channel)

#### Scenario: Upstream #7298 convergence semantics retained

- **WHEN** a permitted removal is applied
- **THEN** upstream kick side effects still fire at convergence and target
  persistence from `ce41be9c4` is unchanged

### Requirement: INV-DESK-02 Managed-node command whitelist

`buzz_managed_command_path` SHALL resolve only whitelisted managed commands
(`node`, `npm`, `npx`, `buzz-pi-acp`, `pi`, `omp`, `opencode`, `deepseek-acp`,
`dsh`, and the pre-existing set), SHALL reject any command containing a path
separator, and SHALL reject non-whitelisted names such as `curl` or `bash`.

#### Scenario: Non-whitelisted command

- **WHEN** `buzz_managed_command_path("bash", "bash")` is called
- **THEN** it returns `None`
