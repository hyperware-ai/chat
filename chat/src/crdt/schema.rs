use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::types::MessageType;

/// Canonical identifier for a replicated group.
pub type GroupId = String;

/// Deterministic identifier for a thread that belongs to a [`GroupId`].
pub type ThreadId = String;

/// Deterministic identifier for a message that belongs to a [`ThreadId`].
pub type MessageId = String;

/// Represents a node (peer) that can join a group.
pub type NodeId = String;

/// Identifier for a membership rule implementation.
pub type MembershipRuleId = String;

/// Static metadata about a group that needs to stay consistent across replicas.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct GroupMetadata {
    pub name: String,
    pub description: Option<String>,
    pub avatar: Option<String>,
    pub creator_id: String,
    pub created_at: u64,
    pub updated_at: u64,
    pub visibility: GroupVisibility,
    pub default_role_id: String,
    pub root_thread_id: ThreadId,
}

impl GroupMetadata {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        name: impl Into<String>,
        description: Option<String>,
        avatar: Option<String>,
        creator_id: impl Into<String>,
        created_at: u64,
        updated_at: u64,
        visibility: GroupVisibility,
        default_role_id: impl Into<String>,
        root_thread_id: ThreadId,
    ) -> Self {
        Self {
            name: name.into(),
            description,
            avatar,
            creator_id: creator_id.into(),
            created_at,
            updated_at,
            visibility,
            default_role_id: default_role_id.into(),
            root_thread_id,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum GroupVisibility {
    Private,
    InviteOnly,
    Public,
}

impl Default for GroupVisibility {
    fn default() -> Self {
        GroupVisibility::Private
    }
}

/// Tier communicates whether a role participates as a hub operator or a regular subscriber.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum GroupTier {
    Hub,
    Subscriber,
}

impl Default for GroupTier {
    fn default() -> Self {
        GroupTier::Subscriber
    }
}

/// Simple bitset wrapper so we can extend permissions without changing serde layout.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct GroupPermissions(pub u64);

impl Default for GroupPermissions {
    fn default() -> Self {
        GroupPermissions(0)
    }
}

impl GroupPermissions {
    pub const SEND_MESSAGES: u64 = 1 << 0;
    pub const CREATE_THREADS: u64 = 1 << 1;
    pub const INVITE_MEMBERS: u64 = 1 << 2;
    pub const MANAGE_ROLES: u64 = 1 << 3;
    pub const MANAGE_SETTINGS: u64 = 1 << 4;

    pub fn empty() -> Self {
        GroupPermissions(0)
    }

    pub fn all() -> Self {
        GroupPermissions(
            Self::SEND_MESSAGES
                | Self::CREATE_THREADS
                | Self::INVITE_MEMBERS
                | Self::MANAGE_ROLES
                | Self::MANAGE_SETTINGS,
        )
    }

    pub fn contains(self, flag: u64) -> bool {
        self.0 & flag == flag
    }

    pub fn insert(&mut self, flag: u64) {
        self.0 |= flag;
    }

    pub fn remove(&mut self, flag: u64) {
        self.0 &= !flag;
    }
}

/// Describes a named role within a group and the permissions it grants.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct GroupRole {
    pub id: String,
    pub label: String,
    pub permissions: GroupPermissions,
    pub tier: GroupTier,
}

impl GroupRole {
    pub fn new(
        id: impl Into<String>,
        label: impl Into<String>,
        permissions: GroupPermissions,
        tier: GroupTier,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            permissions,
            tier,
        }
    }
}

/// Tracks the membership lifecycle for a node.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum MembershipStatus {
    Pending,
    Active,
    Removed,
}

impl Default for MembershipStatus {
    fn default() -> Self {
        MembershipStatus::Pending
    }
}

/// Stores the role binding and recency metadata for a specific member.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct GroupMember {
    pub node_id: NodeId,
    pub role_id: String,
    pub status: MembershipStatus,
    pub last_activity: u64,
}

impl GroupMember {
    pub fn new(
        node_id: impl Into<NodeId>,
        role_id: impl Into<String>,
        status: MembershipStatus,
        last_activity: u64,
    ) -> Self {
        Self {
            node_id: node_id.into(),
            role_id: role_id.into(),
            status,
            last_activity,
        }
    }
}

/// Tracks hub nodes responsible for routing group traffic.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct GroupHubSet {
    #[serde(default)]
    pub active: HashSet<NodeId>,
    #[serde(default)]
    pub pending: HashSet<NodeId>,
}

impl GroupHubSet {
    pub fn new(active: HashSet<NodeId>, pending: HashSet<NodeId>) -> Self {
        Self { active, pending }
    }
}

/// Captures subscriber sync info so routing policies can avoid scanning full members list.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct GroupSubscriberSet {
    #[serde(default)]
    pub entries: HashMap<NodeId, SubscriberSyncState>,
}

impl GroupSubscriberSet {
    pub fn upsert(&mut self, node_id: NodeId, state: SubscriberSyncState) {
        self.entries.insert(node_id, state);
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SubscriberSyncState {
    #[serde(default)]
    pub last_state_vector: Option<Vec<u8>>,
    pub last_snapshot_digest: Option<String>,
    pub last_seen_ts: u64,
}

/// Declarative configuration for membership rules so compiled strategies stay deterministic.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct MembershipRuleConfig {
    pub rule_id: MembershipRuleId,
    #[serde(default)]
    pub params: Value,
}

impl MembershipRuleConfig {
    pub fn new(rule_id: impl Into<MembershipRuleId>, params: Value) -> Self {
        Self {
            rule_id: rule_id.into(),
            params,
        }
    }
}

/// Identifies the parent of a thread (either the group root or another thread).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum ThreadParentRef {
    Root(GroupId),
    Thread(ThreadId),
}

/// Lightweight summary so clients can render thread previews.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ThreadSummary {
    pub message_count: u64,
    pub last_message_id: Option<MessageId>,
    pub last_activity: u64,
    pub last_sender: Option<NodeId>,
}

/// Represents a thread (root or nested) within a group.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct GroupThread {
    pub id: ThreadId,
    pub group_id: GroupId,
    pub depth: u32,
    pub parent: ThreadParentRef,
    pub child_threads: Vec<ThreadId>,
    pub created_at: u64,
    pub created_by: NodeId,
    pub root_message_id: Option<MessageId>,
    pub summary: ThreadSummary,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub archived: bool,
}

impl GroupThread {
    pub fn new(
        id: ThreadId,
        group_id: GroupId,
        depth: u32,
        parent: ThreadParentRef,
        created_at: u64,
        created_by: NodeId,
    ) -> Self {
        Self {
            id,
            group_id,
            depth,
            parent,
            child_threads: Vec::new(),
            created_at,
            created_by,
            root_message_id: None,
            summary: ThreadSummary::default(),
            title: None,
            archived: false,
        }
    }
}

/// Minimal attachment descriptor so replicas agree on included assets without full payloads.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AttachmentDescriptor {
    pub attachment_id: String,
    pub filename: String,
    pub mime_type: String,
    pub size_bytes: u64,
    pub checksum: Option<String>,
    pub uri: Option<String>,
}

/// CRDT-friendly description of a group message without heavyweight content blobs.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MessageMeta {
    pub message_id: MessageId,
    pub thread_id: ThreadId,
    pub group_id: GroupId,
    pub sender: NodeId,
    pub timestamp: u64,
    pub message_type: MessageType,
    #[serde(default)]
    pub reply_to: Option<MessageId>,
    #[serde(default)]
    pub reply_in_thread: Option<MessageId>,
    #[serde(default)]
    pub reactions: Vec<MessageReactionMeta>,
    #[serde(default)]
    pub attachments: Vec<AttachmentDescriptor>,
}

impl MessageMeta {
    pub fn new(
        message_id: MessageId,
        thread_id: ThreadId,
        group_id: GroupId,
        sender: NodeId,
        timestamp: u64,
        message_type: MessageType,
    ) -> Self {
        Self {
            message_id,
            thread_id,
            group_id,
            sender,
            timestamp,
            message_type,
            reply_to: None,
            reply_in_thread: None,
            reactions: Vec::new(),
            attachments: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MessageReactionMeta {
    pub node_id: NodeId,
    pub emoji: String,
    pub timestamp: u64,
}

/// Tracks monotonically increasing counters per group so CRDT peers agree on thread/message IDs.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct StableIdAllocator {
    #[serde(default)]
    per_group: HashMap<GroupId, GroupCounters>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
struct GroupCounters {
    next_thread: u64,
    next_message: u64,
}

impl StableIdAllocator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the next stable thread ID for the provided group.
    pub fn next_thread_id(&mut self, group_id: &GroupId) -> ThreadId {
        let counters = self.counters_mut(group_id);
        let id = Self::format_thread_id(group_id, counters.next_thread);
        counters.next_thread += 1;
        id
    }

    /// Returns the next stable message ID for the provided group.
    pub fn next_message_id(&mut self, group_id: &GroupId) -> MessageId {
        let counters = self.counters_mut(group_id);
        let id = Self::format_message_id(group_id, counters.next_message);
        counters.next_message += 1;
        id
    }

    /// Peeks at the counter that will be used for the next thread ID without mutating state.
    pub fn peek_thread_counter(&self, group_id: &GroupId) -> u64 {
        self.per_group
            .get(group_id)
            .map(|counters| counters.next_thread)
            .unwrap_or(0)
    }

    /// Peeks at the counter that will be used for the next message ID without mutating state.
    pub fn peek_message_counter(&self, group_id: &GroupId) -> u64 {
        self.per_group
            .get(group_id)
            .map(|counters| counters.next_message)
            .unwrap_or(0)
    }

    fn counters_mut(&mut self, group_id: &GroupId) -> &mut GroupCounters {
        self.per_group
            .entry(group_id.clone())
            .or_insert_with(GroupCounters::default)
    }

    fn format_thread_id(group_id: &GroupId, counter: u64) -> ThreadId {
        format!("{group_id}:thread:{counter}")
    }

    fn format_message_id(group_id: &GroupId, counter: u64) -> MessageId {
        format!("{group_id}:msg:{counter}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thread_ids_are_unique_per_group() {
        let mut allocator = StableIdAllocator::new();
        let group = "group-a".to_string();
        let first = allocator.next_thread_id(&group);
        let second = allocator.next_thread_id(&group);

        assert_ne!(first, second);
        assert_eq!(allocator.peek_thread_counter(&group), 2);
        assert!(first.starts_with("group-a:thread:"));
    }

    #[test]
    fn message_ids_are_isolated_between_groups() {
        let mut allocator = StableIdAllocator::new();
        let group_a = "group-a".to_string();
        let group_b = "group-b".to_string();

        let msg_a = allocator.next_message_id(&group_a);
        let msg_b = allocator.next_message_id(&group_b);

        assert_ne!(msg_a, msg_b);
        assert!(msg_a.starts_with("group-a:msg:"));
        assert!(msg_b.starts_with("group-b:msg:"));
        assert_eq!(allocator.peek_message_counter(&group_a), 1);
        assert_eq!(allocator.peek_message_counter(&group_b), 1);
    }
}
