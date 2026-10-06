## MODIFIED Requirements

### Requirement: INV-ACP-03 Direct reply enforcement and native steer reconciliation

When `BUZZ_REPLY_IN_THREAD=false` (or direct-reply mode is enforced), the harness SHALL compute fallback thread tags that do not thread the reply (`queue.rs::fallback_thread_tags`), while co-existing with upstream native steering trigger detection (`queue.rs::reply_anchor_is_trigger`).

#### Scenario: Direct reply configured with native steer support

- **GIVEN** `BUZZ_REPLY_IN_THREAD=false` or direct-reply enforcement is active
- **WHEN** a channel message triggers a prompt task in `pool.rs`
- **THEN** `publication_thread_tags` carry no thread root for fallback output
- **AND** `prompt_routing.record_trigger_anchor` accurately records trigger anchor status for native steer guards

#### Scenario: Fallback thread tags calculation

- **GIVEN** a trigger event with existing thread tags
- **WHEN** `fallback_thread_tags` is evaluated in direct mode
- **THEN** parent and root event IDs are cleared so the reply lands directly at the channel root
