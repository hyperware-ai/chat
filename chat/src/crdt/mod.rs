use std::collections::HashMap;

use hyperware_crdt::yrs::StateVector;
use hyperware_crdt::{CommitteeDoc, CommitteeError};
use serde::{Deserialize, Serialize};

use crate::types::{Chat, ChatKey, ChatState, Settings, UserProfile};

pub mod schema;
pub use schema::compile_membership_rules;
pub use schema::{
    AttachmentDescriptor, DictatorRule, Group, GroupCounters, GroupHubSet, GroupId, GroupMember,
    GroupMetadata, GroupPermissions, GroupSubscriberSet, GroupTier, GroupVisibility,
    MembershipActionKind, MembershipDecision, MembershipDecisionStatus, MembershipProposal,
    MembershipRule, MembershipRuleBox, MembershipRuleConfig, MembershipRuleError,
    MembershipRuleId, MembershipStatus, MessageId, MessageMeta, MessageReactionMeta,
    MultiDictatorRule, NodeId, Role, SubscriberSyncState, TallyVoteRule, Thread, ThreadId,
    ThreadParentRef, ThreadSummary, TokenThresholdRule,
};

pub const CHAT_DOC_ID: &str = "chat:dm_state";

/// Wraps the committee document plus helper metadata (like last state vectors).
pub struct ChatCrdtManager {
    doc: CommitteeDoc<ChatDocState>,
    last_state_vector: Option<StateVector>,
}

impl ChatCrdtManager {
    pub fn new(doc_id: impl Into<String>, runtime: &ChatState) -> Result<Self, CommitteeError> {
        let snapshot = ChatDocState::from(runtime);
        Self::from_snapshot(doc_id, snapshot)
    }

    pub fn from_snapshot(
        doc_id: impl Into<String>,
        snapshot: ChatDocState,
    ) -> Result<Self, CommitteeError> {
        let doc = CommitteeDoc::new(doc_id, snapshot)?;
        let last_state_vector = Some(doc.state_vector());
        Ok(Self {
            doc,
            last_state_vector,
        })
    }

    pub fn doc(&self) -> &CommitteeDoc<ChatDocState> {
        &self.doc
    }

    pub fn doc_mut(&mut self) -> &mut CommitteeDoc<ChatDocState> {
        &mut self.doc
    }

    pub fn last_state_vector(&self) -> Option<&StateVector> {
        self.last_state_vector.as_ref()
    }

    pub fn refresh_from(&mut self, runtime: &ChatState) -> Result<(), CommitteeError> {
        let snapshot = ChatDocState::from(runtime);
        self.refresh_with_snapshot(snapshot)
    }

    pub fn refresh_with_snapshot(&mut self, snapshot: ChatDocState) -> Result<(), CommitteeError> {
        self.doc.write_state(&snapshot)?;
        self.last_state_vector = Some(self.doc.state_vector());
        Ok(())
    }

    pub fn set_last_state_vector(&mut self, sv: StateVector) {
        self.last_state_vector = Some(sv);
    }
}

/// Per-group CRDT manager responsible for syncing a single [`GroupId`] document.
pub struct GroupCrdtManager {
    group_id: GroupId,
    doc: CommitteeDoc<GroupDocState>,
    last_state_vector: Option<StateVector>,
}

impl GroupCrdtManager {
    pub fn doc_id(group_id: &GroupId) -> String {
        format!("chat:group:{group_id}")
    }

    pub fn from_group(group_id: &GroupId, group: &Group) -> Result<Self, CommitteeError> {
        let snapshot: GroupDocState = (group_id, group).into();
        Self::from_snapshot(snapshot)
    }

    pub fn from_snapshot(snapshot: GroupDocState) -> Result<Self, CommitteeError> {
        let group_id = snapshot.group_id.clone();
        let doc = CommitteeDoc::new(Self::doc_id(&group_id), snapshot)?;
        let last_state_vector = Some(doc.state_vector());
        Ok(Self {
            group_id,
            doc,
            last_state_vector,
        })
    }

    pub fn doc(&self) -> &CommitteeDoc<GroupDocState> {
        &self.doc
    }

    pub fn doc_mut(&mut self) -> &mut CommitteeDoc<GroupDocState> {
        &mut self.doc
    }

    pub fn group_id(&self) -> &GroupId {
        &self.group_id
    }

    pub fn last_state_vector(&self) -> Option<&StateVector> {
        self.last_state_vector.as_ref()
    }

    pub fn refresh_from_group(&mut self, group: &Group) -> Result<(), CommitteeError> {
        let snapshot: GroupDocState = (&self.group_id, group).into();
        self.refresh_with_snapshot(snapshot)
    }

    pub fn refresh_with_snapshot(&mut self, snapshot: GroupDocState) -> Result<(), CommitteeError> {
        if snapshot.group_id != self.group_id {
            return Err(CommitteeError::Observer(format!(
                "snapshot group mismatch: expected {} got {}",
                self.group_id, snapshot.group_id
            )));
        }
        self.doc.write_state(&snapshot)?;
        self.last_state_vector = Some(self.doc.state_vector());
        Ok(())
    }

    pub fn set_last_state_vector(&mut self, sv: StateVector) {
        self.last_state_vector = Some(sv);
    }
}

/// Snapshot of the chat application state that is safe to replicate via CRDT.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
pub struct ChatDocState {
    pub profile: UserProfile,
    pub chats: HashMap<String, Chat>,
    pub chat_keys: HashMap<String, ChatKey>,
    pub settings: Settings,
    #[serde(default)]
    pub message_sequence_counters: HashMap<String, u64>,
    #[serde(default)]
    pub node_profiles: HashMap<String, UserProfile>,
}

impl From<&ChatState> for ChatDocState {
    fn from(runtime: &ChatState) -> Self {
        ChatDocState {
            profile: runtime.profile.clone(),
            chats: runtime.chats.clone(),
            chat_keys: runtime.chat_keys.clone(),
            settings: runtime.settings.clone(),
            message_sequence_counters: runtime.message_sequence_counters.clone(),
            node_profiles: runtime.node_profiles.clone(),
        }
    }
}

impl ChatDocState {
    /// Apply the replicated state to the runtime while leaving ephemeral fields untouched.
    pub fn apply_into(&self, runtime: &mut ChatState) {
        runtime.profile = self.profile.clone();
        runtime.chats = self.chats.clone();
        runtime.chat_keys = self.chat_keys.clone();
        runtime.settings = self.settings.clone();
        runtime.message_sequence_counters = self.message_sequence_counters.clone();
        runtime.node_profiles = self.node_profiles.clone();
    }
}

/// Snapshot for a single group that replicates its metadata, members, and content.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
pub struct GroupDocState {
    pub group_id: GroupId,
    #[serde(default)]
    pub group: Group,
}

impl GroupDocState {
    pub fn new(group_id: GroupId, group: Group) -> Self {
        Self { group_id, group }
    }

    pub fn apply_into(&self, runtime: &mut ChatState) {
        runtime.groups.insert(self.group_id.clone(), self.group.clone());
        runtime.invalidate_group_rules(&self.group_id);
    }
}

impl From<(&GroupId, &Group)> for GroupDocState {
    fn from((group_id, group): (&GroupId, &Group)) -> Self {
        GroupDocState::new(group_id.clone(), group.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::{ChatDocState, GroupCrdtManager, GroupDocState};
    use crate::crdt::{Group, GroupCounters};
    use crate::{ChatState, MessageStatus};

    #[test]
    fn apply_round_trip_preserves_chats() {
        let mut runtime = ChatState::default();
        runtime.chats.insert(
            "alice:bob".into(),
            crate::Chat {
                id: "alice:bob".into(),
                counterparty: "bob".into(),
                messages: vec![crate::ChatMessage {
                    id: "msg".into(),
                    sender: "alice".into(),
                    content: "hi".into(),
                    timestamp: 1,
                    sequence: Some(1),
                    status: MessageStatus::Sent,
                    reply_to: None,
                    reactions: vec![],
                    message_type: crate::MessageType::Text,
                    file_info: None,
                }],
                last_activity: 1,
                unread_count: 0,
                is_blocked: false,
                notify: true,
                counterparty_profile: None,
            },
        );

        let doc: ChatDocState = (&runtime).into();
        let mut other = ChatState::default();
        doc.apply_into(&mut other);

        assert_eq!(runtime.chats, other.chats);
        assert_eq!(runtime.profile, other.profile);
    }

    #[test]
    fn group_manager_round_trip_writes_state() {
        let group_id = "group:test".to_string();
        let mut group = Group::default();
        group.counters = GroupCounters {
            next_thread: 3,
            next_message: 2,
        };
        let manager = GroupCrdtManager::from_group(&group_id, &group)
            .expect("failed to build group manager");

        let doc_state = manager
            .doc()
            .read_state()
            .expect("failed to read doc state");
        assert_eq!(doc_state.group_id, group_id);
        assert_eq!(doc_state.group.counters.next_thread, 3);

        let mut updated_group = group.clone();
        updated_group.counters.next_thread = 5;
        let mut manager = manager;
        manager
            .refresh_from_group(&updated_group)
            .expect("refresh_from_group should succeed");
        let refreshed = manager
            .doc()
            .read_state()
            .expect("failed to read refreshed doc");
        assert_eq!(refreshed.group.counters.next_thread, 5);
    }

    #[test]
    fn group_doc_state_apply_preserves_counters() {
        let mut runtime = ChatState::default();
        let mut group = Group::default();
        group.counters = GroupCounters {
            next_thread: 4,
            next_message: 9,
        };
        let snapshot = GroupDocState::new("group:apply".to_string(), group);
        snapshot.apply_into(&mut runtime);

        let stored = runtime
            .groups
            .get("group:apply")
            .expect("group should be present");
        assert_eq!(stored.counters.next_thread, 4);
        assert_eq!(stored.counters.next_message, 9);
    }
}
