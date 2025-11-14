use std::collections::HashMap;

use hyperware_crdt::yrs::StateVector;
use hyperware_crdt::{CommitteeDoc, CommitteeError};
use serde::{Deserialize, Serialize};

use crate::types::{Chat, ChatKey, ChatState, Settings, UserProfile};

pub mod schema;
pub use schema::{
    AttachmentDescriptor, GroupHubSet, GroupId, GroupMember, GroupMetadata, GroupPermissions,
    GroupRole, GroupSubscriberSet, GroupThread, GroupTier, GroupVisibility, MembershipRuleConfig,
    MembershipRuleId, MembershipStatus, MessageId, MessageMeta, MessageReactionMeta, NodeId,
    StableIdAllocator, SubscriberSyncState, ThreadId, ThreadParentRef, ThreadSummary,
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

/// Snapshot of the chat application state that is safe to replicate via CRDT.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ChatDocState {
    pub profile: UserProfile,
    pub chats: HashMap<String, Chat>,
    pub chat_keys: HashMap<String, ChatKey>,
    pub settings: Settings,
    #[serde(default)]
    pub message_sequence_counters: HashMap<String, u64>,
    #[serde(default)]
    pub node_profiles: HashMap<String, UserProfile>,
    #[serde(default)]
    pub groups: GroupDocState,
}

impl Default for ChatDocState {
    fn default() -> Self {
        ChatDocState {
            profile: UserProfile::default(),
            chats: HashMap::new(),
            chat_keys: HashMap::new(),
            settings: Settings::default(),
            message_sequence_counters: HashMap::new(),
            node_profiles: HashMap::new(),
            groups: GroupDocState::default(),
        }
    }
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
            groups: GroupDocState::default(),
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

/// Placeholder for upcoming group-chat replication data.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct GroupDocState {
    #[serde(default)]
    pub metadata: HashMap<GroupId, GroupMetadata>,
    #[serde(default)]
    pub roles: HashMap<GroupId, HashMap<String, GroupRole>>,
    #[serde(default)]
    pub members: HashMap<GroupId, HashMap<NodeId, GroupMember>>,
    #[serde(default)]
    pub hubs: HashMap<GroupId, GroupHubSet>,
    #[serde(default)]
    pub subscribers: HashMap<GroupId, GroupSubscriberSet>,
    #[serde(default)]
    pub membership_rules: HashMap<GroupId, Vec<MembershipRuleConfig>>,
    #[serde(default)]
    pub threads: HashMap<ThreadId, GroupThread>,
    #[serde(default)]
    pub messages: HashMap<MessageId, MessageMeta>,
    #[serde(default)]
    pub stable_id_allocator: StableIdAllocator,
}

#[cfg(test)]
mod tests {
    use super::ChatDocState;
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
}
