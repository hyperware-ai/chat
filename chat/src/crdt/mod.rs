use crate::ChatState;
use hyperware_crdt::yrs::StateVector;
use hyperware_crdt::{CommitteeDoc, CommitteeError};
use serde::{Deserialize, Serialize};

pub mod schema;
pub use schema::compile_membership_rules;
#[allow(unused_imports)]
pub use schema::{
    AttachmentDescriptor, DeliveryCursor, DictatorRule, Group, GroupCounters, GroupDeliveryState,
    GroupHubSet, GroupId, GroupMember, GroupMetadata, GroupPermissions, GroupRoutingConfig,
    GroupSubscriberSet, GroupTier, GroupVisibility, HubSyncState, MembershipActionKind,
    MembershipDecision, MembershipDecisionStatus, MembershipProposal, MembershipRule,
    MembershipRuleBox, MembershipRuleConfig, MembershipRuleError, MembershipRuleId,
    MembershipStatus, MessageId, MessageMeta, MessageReactionMeta, MultiDictatorRule, NodeId, Role,
    SubscriberSyncState, TallyVoteRule, Thread, ThreadId, ThreadParentRef, ThreadSummary,
    TokenThresholdRule,
};

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
        runtime
            .groups
            .insert(self.group_id.clone(), self.group.clone());
        runtime.invalidate_group_rules(&self.group_id);
        runtime.rebuild_pubsub_for_group(&self.group_id);
    }
}

impl From<(&GroupId, &Group)> for GroupDocState {
    fn from((group_id, group): (&GroupId, &Group)) -> Self {
        GroupDocState::new(group_id.clone(), group.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::{GroupCrdtManager, GroupDocState};
    use crate::crdt::{
        Group, GroupCounters, GroupMember, GroupPermissions, GroupRoutingConfig, GroupTier, Role,
        SubscriberSyncState, MembershipStatus,
    };
    use crate::ChatState;
    use hyperware_pubsub_core::{TopicId as BrokerTopicId, whitelist::NodeId as BrokerNodeId};
    use std::time::SystemTime;

    #[test]
    fn group_manager_round_trip_writes_state() {
        let group_id = "group:test".to_string();
        let mut group = Group::default();
        group.counters = GroupCounters {
            next_thread: 3,
            next_message: 2,
        };
        let manager =
            GroupCrdtManager::from_group(&group_id, &group).expect("failed to build group manager");

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

    #[test]
    fn apply_into_rebuilds_pubsub_acl() {
        let mut runtime = ChatState::default();
        let group_id = "group:acl".to_string();
        let mut group = Group::default();
        group.routing = GroupRoutingConfig::for_group(&group_id);

        let mut perms = GroupPermissions::empty();
        perms.insert(GroupPermissions::SEND_MESSAGES);
        perms.insert(GroupPermissions::CREATE_THREADS);
        let role_id = "role:member".to_string();

        group.roles.insert(
            role_id.clone(),
            Role::new(role_id.clone(), "Member", perms, GroupTier::Subscriber),
        );
        group.members.insert(
            "member.node".into(),
            GroupMember::new(
                "member.node",
                role_id.clone(),
                MembershipStatus::Active,
                0,
            ),
        );
        group
            .subscribers
            .entries
            .insert("member.node".into(), SubscriberSyncState::default());
        group.hubs.active.insert("member.node".into());

        let snapshot = GroupDocState::new(group_id.clone(), group);
        snapshot.apply_into(&mut runtime);

        let whitelist = runtime
            .pubsub
            .whitelist(&group_id)
            .expect("whitelist projected");
        let subscriber_topic = BrokerTopicId::new(
            runtime
                .groups
                .get(&group_id)
                .unwrap()
                .routing
                .subscriber_topic
                .clone(),
        );

        assert!(
            whitelist
                .subscribe_scope(
                    &BrokerNodeId::new("member.node"),
                    &subscriber_topic,
                    SystemTime::now()
                )
                .is_some()
        );
    }
}
