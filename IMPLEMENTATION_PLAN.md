Federated Hub-and-Spoke Chat: Current State vs Next Steps

What’s Implemented
- CRDT foundation: CommitteeDoc for groups; schema covers metadata, roles, hubs, subscribers, routing, delivery cursors, threads/messages with stable IDs; state-vector tracking and apply/read; pubsub whitelist projection rebuilt on apply/commit.
- Membership & roles: Group/GroupMember/Role tiers with permissions; compiled rules (dictator, multi-dictator, tally, token placeholder); invite/remove flows update hub/sub sets and CRDT; permissions enforced on thread creation, group messaging, and membership actions.
- ACL projection/enforcement: Whitelist derived from roles/subscribers; hub publish/sub checks on CRDT endpoints and inbound apply; group list/get filtered by subscriber access; fanout skips hubs lacking subscribe rights.
- Replication worker: Internal broker-backed loop drives per-topic hub/sub updates with offsets + retry/backoff, per-target state-vector cursors, ACL-version tagging, delivery cursor updates, and snapshot bootstrap pulls when pending; publisher-side filtering skips peers lacking subscribe rights; subscriber lane replay/TTL/dedupe added with stale-ack replay scheduling.
- Observability/ops scaffolding: Replication metrics (ACL skips, retries, drops, lag, stale replays, ACL drift), per-envelope lag logging, subscriber delivery event buffer, and admin endpoints for replication state/whitelist/events.
- Chat/DM baseline: Existing DM flows, delivery queue, web UI, and push notifications remain intact alongside group structures.
- Group UI/client flows: Create/list/open groups; view threads and send messages; member invite/approval UI with role selection; subscriber lane health surfaced in UI; message bodies stored/returned with group messages so text renders across nodes.

Still Needed for Full P2P Federated Hub-and-Spoke
- UI polish: desktop-first refinements, richer thread creation/navigation, leave/remove member flows, and role management (edit labels/permissions).
- Client surfacing of subscriber lane state in WS: push notifications for lag/health instead of polling; better error/retry UX.
- Alerts/guardrails: wire metrics into alerting, add rate limits for replay scheduling, and guardrail toggles/feature flags for rollout.
- Testing & migration: add tests for replication worker (broker + RPC), bootstrap/resume, ACL version gating, subscriber delivery paths, failure/replay cases; supply migration scripts/feature flags for rollout.

Instruction:
- always run `kit run-tests` after the changes you make and make sure it passes
