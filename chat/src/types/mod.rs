use futures::channel::mpsc::{self, UnboundedReceiver, UnboundedSender};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::crdt::{
    AttachmentDescriptor, DeliveryCursor, Group, GroupCounters, GroupCrdtManager, GroupId,
    GroupMember, GroupMetadata, GroupPermissions, GroupRoutingConfig, GroupTier, GroupVisibility,
    HubSyncState, MembershipActionKind, MembershipDecision, MembershipDecisionStatus,
    MembershipProposal, MembershipRuleBox, MembershipRuleConfig, MembershipRuleError,
    MembershipStatus, MessageId, MessageMeta, NodeId, Role, SubscriberSyncState, Thread, ThreadId,
    ThreadParentRef,
};
use crate::pubsub::PubSubRegistry;
use hyperware_crdt::CommitteeError;
use hyperware_process_lib::our;
use hyperware_pubsub_core::{whitelist::NodeId as BrokerNodeId, TopicId as BrokerTopicId};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PushSubscription {
    pub endpoint: String,
    pub keys: SubscriptionKeys,
}

pub(crate) fn aggregate_rule_decisions(
    rules: &[MembershipRuleBox],
    proposal: &MembershipProposal,
) -> MembershipDecision {
    if rules.is_empty() {
        return MembershipDecision::approved();
    }

    let mut pending: HashSet<NodeId> = HashSet::new();
    for rule in rules {
        let decision = rule.evaluate(proposal);
        match decision.status {
            MembershipDecisionStatus::Approved => {}
            MembershipDecisionStatus::Pending => {
                for sig in decision.missing_signatures {
                    pending.insert(sig);
                }
            }
            MembershipDecisionStatus::Rejected => return decision,
        }
    }

    if pending.is_empty() {
        MembershipDecision::approved()
    } else {
        let mut missing: Vec<NodeId> = pending.into_iter().collect();
        missing.sort();
        MembershipDecision::pending(missing)
    }
}

pub(crate) fn active_member_count(group: &Group) -> u32 {
    group
        .members
        .values()
        .filter(|member| member.status == MembershipStatus::Active)
        .count() as u32
}

pub(crate) fn membership_proposal_key(
    group_id: &GroupId,
    candidate: &NodeId,
    action: MembershipActionKind,
) -> String {
    let action_str = match action {
        MembershipActionKind::Invite => "invite",
        MembershipActionKind::Remove => "remove",
    };
    format!("{group_id}:{action_str}:{candidate}")
}

pub(crate) fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn default_group_message_type() -> MessageType {
    MessageType::Text
}

fn ensure_membership_rules(
    mut rules: Vec<MembershipRuleConfig>,
    creator: &NodeId,
) -> Vec<MembershipRuleConfig> {
    if rules.is_empty() {
        rules = default_membership_rules(creator);
    }
    rules
}

fn default_membership_rules(creator: &NodeId) -> Vec<MembershipRuleConfig> {
    vec![MembershipRuleConfig::new(
        "membership.rule.dictator",
        json!({ "dictator": creator }),
    )]
}

pub(crate) fn group_root_thread_id(group: &Group) -> Option<ThreadId> {
    group
        .metadata
        .as_ref()
        .map(|metadata| metadata.root_thread_id.clone())
}

pub(crate) fn sync_member_membership_sets(group: &mut Group, member_id: &NodeId, timestamp: u64) {
    let Some(member) = group.members.get(member_id) else {
        group.hubs.active.remove(member_id);
        group.subscribers.entries.remove(member_id);
        return;
    };

    if member.status == MembershipStatus::Removed {
        group.hubs.active.remove(member_id);
        group.subscribers.entries.remove(member_id);
        return;
    }

    if let Some(role) = group.roles.get(&member.role_id) {
        match role.tier {
            GroupTier::Hub => {
                group.hubs.active.insert(member_id.clone());
            }
            GroupTier::Subscriber => {
                group.hubs.active.remove(member_id);
            }
        }
    }

    let state = group
        .subscribers
        .entries
        .entry(member_id.clone())
        .or_insert_with(SubscriberSyncState::default);
    state.last_seen_ts = timestamp;
}

fn generate_group_id() -> GroupId {
    let timestamp = current_timestamp();
    let nonce: u32 = rand::random();
    format!("group:{}:{}:{}", our().node, timestamp, nonce)
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SubscriptionKeys {
    pub p256dh: String,
    pub auth: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum NotificationsAction {
    SendNotification {
        title: String,
        body: String,
        icon: Option<String>,
        data: Option<serde_json::Value>,
    },
    GetPublicKey,
    InitializeKeys,
    AddSubscription {
        subscription: PushSubscription,
    },
    RemoveSubscription {
        endpoint: String,
    },
    ClearSubscriptions,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum NotificationsResponse {
    NotificationSent,
    PublicKey(String),
    KeysInitialized,
    SubscriptionAdded,
    SubscriptionRemoved,
    SubscriptionsCleared,
    Err(String),
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ChatMessage {
    pub id: String,
    pub sender: String,
    pub content: String,
    pub timestamp: u64,
    #[serde(default)]
    pub sequence: Option<u64>,
    pub status: MessageStatus,
    pub reply_to: Option<String>,
    pub reactions: Vec<MessageReaction>,
    pub message_type: MessageType,
    pub file_info: Option<FileInfo>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct MessageReaction {
    pub emoji: String,
    pub user: String,
    pub timestamp: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum MessageType {
    Text,
    Image,
    File,
    VoiceNote,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct FileInfo {
    pub filename: String,
    pub mime_type: String,
    pub size: u64,
    pub url: String,
}

/// MessageStatus lifecycle: outbound messages start at `Sending`, flip to `Sent`
/// once stored locally, move to `Delivered` on ack/receipt from the counterparty,
/// and may be marked `Failed` by delivery retries if a destination remains unreachable.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum MessageStatus {
    Sending,
    Sent,
    Delivered,
    Failed,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Chat {
    pub id: String,
    pub counterparty: String,
    pub messages: Vec<ChatMessage>,
    pub last_activity: u64,
    pub unread_count: u32,
    pub is_blocked: bool,
    pub notify: bool,
    #[serde(default)]
    pub counterparty_profile: Option<UserProfile>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ChatKey {
    pub key: String,
    pub user_name: String,
    pub created_at: u64,
    pub is_revoked: bool,
    pub chat_id: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct UserProfile {
    pub name: String,
    pub profile_pic: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Settings {
    pub show_images: bool,
    pub show_profile_pics: bool,
    pub combine_chats_groups: bool,
    pub notify_chats: bool,
    pub notify_groups: bool,
    pub notify_calls: bool,
    pub allow_browser_chats: bool,
    pub stt_enabled: bool,
    pub stt_api_key: Option<String>,
    pub max_file_size_mb: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            show_images: true,
            show_profile_pics: true,
            combine_chats_groups: false,
            notify_chats: true,
            notify_groups: true,
            notify_calls: true,
            allow_browser_chats: true,
            stt_enabled: false,
            stt_api_key: None,
            max_file_size_mb: 10,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum WsClientMessage {
    SendMessage {
        chat_id: String,
        content: String,
        reply_to: Option<String>,
    },
    Ack {
        message_id: String,
    },
    MarkRead {
        chat_id: String,
    },
    UpdateStatus {
        status: String,
    },
    AuthWithKey {
        chat_key: String,
    },
    BrowserMessage {
        content: String,
    },
    Heartbeat,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum WsServerMessage {
    NewMessage(ChatMessage),
    MessageAck {
        message_id: String,
    },
    StatusUpdate {
        node: String,
        status: String,
    },
    ChatUpdate(Chat),
    ProfileUpdate {
        node: String,
        profile: UserProfile,
    },
    AuthSuccess {
        chat_id: String,
        history: Vec<ChatMessage>,
    },
    AuthFailed {
        reason: String,
    },
    Heartbeat,
    Error {
        message: String,
    },
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ReplicationMetrics {
    #[serde(default)]
    pub acl_skips: u64,
    #[serde(default)]
    pub retries: u64,
    #[serde(default)]
    pub drops: u64,
    #[serde(default)]
    pub stale_replays: u64,
    #[serde(default)]
    pub last_lag_secs: u64,
    #[serde(default)]
    pub last_subscriber_lag_secs: u64,
    #[serde(default)]
    pub acl_drifts: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SubscriberDeliveryEvent {
    pub group_id: GroupId,
    pub topic: String,
    pub offset: u64,
    pub kind: ReplicationKind,
    pub age_secs: u64,
    pub recorded_at: u64,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CreateChatReq {
    pub counterparty: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct GetChatReq {
    pub chat_id: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct GetMessagesReq {
    pub chat_id: String,
    pub before_timestamp: Option<u64>,
    pub limit: Option<u64>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct GetSyncHashReq {
    pub chat_id: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SyncHashInfo {
    pub chat_id: String,
    pub message_count: u32,
    pub last_message_id: Option<String>,
    pub last_message_timestamp: Option<u64>,
    pub hash: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct DeleteChatReq {
    pub chat_id: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SendMessageReq {
    pub chat_id: String,
    pub content: String,
    pub reply_to: Option<String>,
    pub file_info: Option<FileInfo>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct EditMessageReq {
    pub chat_id: String,
    pub message_id: String,
    pub new_content: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct DeleteMessageReq {
    pub chat_id: String,
    pub message_id: String,
    pub delete_for_both: Option<bool>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct AddReactionReq {
    pub chat_id: String,
    pub message_id: String,
    pub emoji: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct RemoveReactionReq {
    pub chat_id: String,
    pub message_id: String,
    pub emoji: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ForwardMessageReq {
    pub from_chat_id: String,
    pub message_id: String,
    pub to_chat_id: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CreateChatLinkReq {
    pub chat_id: String,
    pub single_use: bool,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct RevokeChatKeyReq {
    pub key: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct UploadFileReq {
    pub chat_id: String,
    pub filename: String,
    pub mime_type: String,
    pub data: String,
    pub reply_to: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct UploadProfilePictureReq {
    pub mime_type: String,
    pub data: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SendVoiceNoteReq {
    pub chat_id: String,
    pub audio_data: String,
    pub duration: u32,
    pub reply_to: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SearchChatsReq {
    pub query: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CreateGroupReq {
    #[serde(default)]
    pub group_id: Option<GroupId>,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub avatar: Option<String>,
    #[serde(default)]
    pub visibility: Option<GroupVisibility>,
    #[serde(default)]
    pub default_role_label: Option<String>,
    #[serde(default)]
    pub membership_rules: Vec<MembershipRuleConfig>,
    #[serde(default)]
    pub root_thread_title: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CreateGroupRes {
    pub group_id: GroupId,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CreateGroupThreadReq {
    pub group_id: GroupId,
    #[serde(default)]
    pub parent_thread_id: Option<ThreadId>,
    #[serde(default)]
    pub title: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CreateGroupThreadRes {
    pub thread_id: ThreadId,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SendGroupMessageReq {
    pub group_id: GroupId,
    #[serde(default)]
    pub thread_id: Option<ThreadId>,
    pub content: String,
    #[serde(default = "default_group_message_type")]
    pub message_type: MessageType,
    #[serde(default)]
    pub reply_to: Option<MessageId>,
    #[serde(default)]
    pub attachments: Vec<AttachmentDescriptor>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SendGroupMessageRes {
    pub message: MessageMeta,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct GetGroupReq {
    pub group_id: GroupId,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct GetGroupRes {
    pub group: Option<Group>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct GroupSummary {
    pub group_id: GroupId,
    pub metadata: Option<GroupMetadata>,
    pub member_count: usize,
    pub thread_count: usize,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ListGroupsRes {
    pub groups: Vec<GroupSummary>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct InviteGroupMemberReq {
    pub group_id: GroupId,
    pub candidate: NodeId,
    pub role_id: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ApproveGroupMembershipReq {
    pub group_id: GroupId,
    pub proposal_id: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct RemoveGroupMemberReq {
    pub group_id: GroupId,
    pub member: NodeId,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct MembershipDecisionRes {
    pub decision: MembershipDecision,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CrdtStateVectorRes {
    pub state_vector: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CrdtGroupStateVectorReq {
    pub group_id: GroupId,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CrdtGroupUpdateReq {
    pub group_id: GroupId,
    #[serde(default)]
    pub state_vector: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CrdtUpdateRes {
    pub doc_id: String,
    pub update_payload: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CrdtGroupApplyReq {
    pub group_id: GroupId,
    pub update_payload: String,
    #[serde(default)]
    pub acl_version: Option<u64>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CrdtGroupSnapshotReq {
    pub group_id: GroupId,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CrdtApplyRes {
    pub applied: bool,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct AdminReplicationStateReq {
    #[serde(default)]
    pub group_id: Option<GroupId>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct GroupReplicationState {
    pub group_id: GroupId,
    pub pending_bootstrap: bool,
    pub routing: GroupRoutingConfig,
    pub hubs: Vec<NodeId>,
    pub subscribers: Vec<NodeId>,
    pub hub_cursors: HashMap<NodeId, DeliveryCursor>,
    pub subscriber_cursors: HashMap<NodeId, DeliveryCursor>,
    #[serde(default)]
    pub whitelist_version: Option<u64>,
    #[serde(default)]
    pub subscriber_lag_secs: Option<u64>,
    #[serde(default)]
    pub hub_lag_secs: Option<u64>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct AdminReplicationStateRes {
    pub metrics: ReplicationMetrics,
    pub groups: Vec<GroupReplicationState>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct AdminWhitelistReq {
    pub group_id: GroupId,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct WhitelistEntryDebug {
    pub node: String,
    pub publish: Vec<String>,
    pub subscribe: Vec<String>,
    pub audiences: Vec<String>,
    pub features: Vec<String>,
    #[serde(default)]
    pub expires_at: Option<u64>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct AdminWhitelistRes {
    pub group_id: GroupId,
    pub version: u64,
    pub entries: Vec<WhitelistEntryDebug>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SubscriberEventsReq {
    #[serde(default)]
    pub take: Option<usize>,
    #[serde(default)]
    pub clear: bool,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SubscriberEventsRes {
    pub events: Vec<SubscriberDeliveryEvent>,
}

#[derive(Serialize, Deserialize, Clone, Debug, process_macros::SerdeJsonInto)]
pub enum HomepageRequest {
    GetPushSubscription,
}

#[derive(Serialize, Deserialize, Clone, Debug, process_macros::SerdeJsonInto)]
pub enum HomepageResponse {
    PushSubscription(Option<String>),
}

#[derive(Debug)]
pub enum MembershipActionError {
    GroupNotFound(GroupId),
    MemberExists(NodeId),
    MemberNotFound(NodeId),
    ProposalExists(String),
    ProposalNotFound(String),
    RuleError(MembershipRuleError),
    PermissionDenied(String),
}

impl From<MembershipRuleError> for MembershipActionError {
    fn from(err: MembershipRuleError) -> Self {
        MembershipActionError::RuleError(err)
    }
}

impl fmt::Display for MembershipActionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MembershipActionError::GroupNotFound(id) => write!(f, "group '{id}' not found"),
            MembershipActionError::MemberExists(node) => {
                write!(f, "member '{node}' already exists in group")
            }
            MembershipActionError::MemberNotFound(node) => {
                write!(f, "member '{node}' not found in group")
            }
            MembershipActionError::ProposalExists(id) => {
                write!(f, "proposal '{id}' already exists")
            }
            MembershipActionError::ProposalNotFound(id) => {
                write!(f, "proposal '{id}' not found")
            }
            MembershipActionError::RuleError(err) => write!(f, "rule error: {}", err),
            MembershipActionError::PermissionDenied(msg) => {
                write!(f, "permission denied: {}", msg)
            }
        }
    }
}

impl std::error::Error for MembershipActionError {}

/// Persisted fields: profile, chats, chat_keys, settings, message_sequence_counters, groups.
/// Runtime-only state (connections, heartbeats, channels, replication queues, caches, pubsub) is
/// skipped during serialization and rebuilt on startup.
#[derive(Serialize)]
pub struct ChatState {
    pub profile: UserProfile,
    pub chats: HashMap<String, Chat>,
    pub chat_keys: HashMap<String, ChatKey>,
    pub settings: Settings,
    #[serde(default)]
    pub message_sequence_counters: HashMap<String, u64>,
    #[serde(skip)]
    pub delivery_tx: DeliveryTx,
    #[serde(skip)]
    pub delivery_rx: Option<UnboundedReceiver<QueuedDelivery>>,
    #[serde(skip)]
    pub replication_tx: ReplicationTx,
    #[serde(skip)]
    pub replication_rx: Option<UnboundedReceiver<ReplicationTask>>,
    #[serde(skip)]
    pub replication_wake_tx: Option<ReplicationWakeTx>,
    #[serde(skip)]
    pub replication_wake_rx: Option<ReplicationWakeRx>,
    #[serde(skip)]
    pub replication_work_inflight: Arc<AtomicBool>,
    #[serde(skip)]
    pub replication_queue: VecDeque<ReplicationTask>,
    #[serde(skip)]
    pub broker_queues: HashMap<String, VecDeque<BrokerEnvelope>>,
    #[serde(skip)]
    pub broker_offsets: HashMap<String, u64>,
    #[serde(skip)]
    pub broker_cursors: HashMap<String, u64>,
    #[serde(skip)]
    pub delivery_dedupe: HashMap<u64, u64>,
    #[serde(skip)]
    pub subscriber_events: VecDeque<SubscriberDeliveryEvent>,
    #[serde(skip)]
    pub replication_metrics: ReplicationMetrics,
    #[serde(skip)]
    pub pending_deliveries: Arc<Mutex<HashMap<String, Vec<ChatMessage>>>>,
    #[serde(skip)]
    pub ws_connections: HashMap<u32, String>,
    #[serde(skip)]
    pub browser_connections: HashMap<String, u32>,
    #[serde(skip)]
    pub last_heartbeat: HashMap<u32, u64>,
    #[serde(skip)]
    pub active_connections: HashSet<u32>,
    #[serde(skip)]
    pub node_profiles: HashMap<String, UserProfile>,
    #[serde(default)]
    pub groups: HashMap<GroupId, Group>,
    #[serde(skip)]
    pub membership_rule_cache: HashMap<GroupId, Vec<MembershipRuleBox>>,
    #[serde(skip)]
    pub group_doc_managers: HashMap<GroupId, GroupCrdtManager>,
    #[serde(skip)]
    pub groups_pending_bootstrap: HashSet<GroupId>,
    #[serde(skip)]
    pub pubsub: PubSubRegistry,
}

impl Default for ChatState {
    fn default() -> Self {
        let (delivery_tx, delivery_rx) = DeliveryTx::new();
        let (replication_tx, replication_rx) = ReplicationTx::new();
        let (replication_wake_tx, replication_wake_rx) = ReplicationWakeTx::new();

        ChatState {
            profile: UserProfile::default(),
            chats: HashMap::new(),
            chat_keys: HashMap::new(),
            settings: Settings::default(),
            message_sequence_counters: HashMap::new(),
            delivery_tx,
            // represents "still available" versus "already consumed"
            delivery_rx: Some(delivery_rx),
            replication_tx,
            replication_rx: Some(replication_rx),
            replication_wake_tx: Some(replication_wake_tx),
            replication_wake_rx: Some(replication_wake_rx),
            replication_work_inflight: Arc::new(AtomicBool::new(false)),
            replication_queue: VecDeque::new(),
            broker_queues: HashMap::new(),
            broker_offsets: HashMap::new(),
            broker_cursors: HashMap::new(),
            delivery_dedupe: HashMap::new(),
            subscriber_events: VecDeque::new(),
            replication_metrics: ReplicationMetrics::default(),
            pending_deliveries: Arc::new(Mutex::new(HashMap::new())),
            ws_connections: HashMap::new(),
            browser_connections: HashMap::new(),
            last_heartbeat: HashMap::new(),
            active_connections: HashSet::new(),
            node_profiles: HashMap::new(),
            groups: HashMap::new(),
            membership_rule_cache: HashMap::new(),
            group_doc_managers: HashMap::new(),
            groups_pending_bootstrap: HashSet::new(),
            pubsub: PubSubRegistry::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub enum DeliveryEvent {
    Message(ChatMessage),
    Flush,
}

#[derive(Clone, Debug)]
pub struct QueuedDelivery {
    pub node: String,
    pub event: DeliveryEvent,
}

impl QueuedDelivery {
    pub fn message(node: String, message: ChatMessage) -> Self {
        Self {
            node,
            event: DeliveryEvent::Message(message),
        }
    }

    pub fn flush(node: String) -> Self {
        Self {
            node,
            event: DeliveryEvent::Flush,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ReplicationKind {
    PushDelta,
    PushSnapshot,
    PullSnapshot,
    PullDelta,
}

impl Default for ReplicationKind {
    fn default() -> Self {
        ReplicationKind::PushDelta
    }
}

#[derive(Clone, Debug)]
pub struct ReplicationTask {
    pub group_id: GroupId,
    pub peer: String,
    pub kind: ReplicationKind,
    pub since: Option<Vec<u8>>,
    pub attempt: u32,
    pub not_before: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BrokerEnvelope {
    pub offset: u64,
    pub payload: String,
    #[serde(default)]
    pub acl_version: Option<u64>,
    #[serde(default)]
    pub kind: ReplicationKind,
    #[serde(default)]
    pub ts: u64,
}

#[derive(Clone)]
pub struct ReplicationTx {
    sender: UnboundedSender<ReplicationTask>,
}

impl ReplicationTx {
    pub fn new() -> (Self, UnboundedReceiver<ReplicationTask>) {
        let (sender, receiver) = mpsc::unbounded();
        (ReplicationTx { sender }, receiver)
    }

    pub fn unbounded_send(
        &self,
        task: ReplicationTask,
    ) -> Result<(), mpsc::TrySendError<ReplicationTask>> {
        self.sender.unbounded_send(task)
    }
}

#[derive(Clone)]
pub struct DeliveryTx {
    sender: UnboundedSender<QueuedDelivery>,
}

#[derive(Clone)]
pub struct ReplicationWakeTx {
    sender: UnboundedSender<()>,
}

pub struct ReplicationWakeRx {
    receiver: UnboundedReceiver<()>,
}

impl DeliveryTx {
    pub fn new() -> (Self, UnboundedReceiver<QueuedDelivery>) {
        let (sender, receiver) = mpsc::unbounded();
        (DeliveryTx { sender }, receiver)
    }

    pub fn unbounded_send(
        &self,
        delivery: QueuedDelivery,
    ) -> Result<(), mpsc::TrySendError<QueuedDelivery>> {
        self.sender.unbounded_send(delivery)
    }
}

impl ReplicationWakeTx {
    pub fn new() -> (Self, ReplicationWakeRx) {
        let (sender, receiver) = mpsc::unbounded();
        (ReplicationWakeTx { sender }, ReplicationWakeRx { receiver })
    }

    pub fn wake(&self) {
        let _ = self.sender.unbounded_send(());
    }
}

impl ReplicationWakeRx {
    pub fn into_stream(self) -> UnboundedReceiver<()> {
        self.receiver
    }
}

impl<'de> Deserialize<'de> for ChatState {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct ChatStateSerde {
            profile: UserProfile,
            chats: HashMap<String, Chat>,
            chat_keys: HashMap<String, ChatKey>,
            settings: Settings,
            #[serde(default)]
            message_sequence_counters: HashMap<String, u64>,
            #[serde(default)]
            groups: HashMap<GroupId, Group>,
        }

        let data = ChatStateSerde::deserialize(deserializer)?;
        let (delivery_tx, delivery_rx) = DeliveryTx::new();
        let (replication_tx, replication_rx) = ReplicationTx::new();
        let (replication_wake_tx, replication_wake_rx) = ReplicationWakeTx::new();

        let mut state = ChatState {
            profile: data.profile,
            chats: data.chats,
            chat_keys: data.chat_keys,
            settings: data.settings,
            message_sequence_counters: data.message_sequence_counters,
            delivery_tx,
            delivery_rx: Some(delivery_rx),
            replication_tx,
            replication_rx: Some(replication_rx),
            replication_wake_tx: Some(replication_wake_tx),
            replication_wake_rx: Some(replication_wake_rx),
            replication_work_inflight: Arc::new(AtomicBool::new(false)),
            replication_queue: VecDeque::new(),
            broker_queues: HashMap::new(),
            broker_offsets: HashMap::new(),
            broker_cursors: HashMap::new(),
            delivery_dedupe: HashMap::new(),
            subscriber_events: VecDeque::new(),
            replication_metrics: ReplicationMetrics::default(),
            pending_deliveries: Arc::new(Mutex::new(HashMap::new())),
            ws_connections: HashMap::new(),
            browser_connections: HashMap::new(),
            last_heartbeat: HashMap::new(),
            active_connections: HashSet::new(),
            node_profiles: HashMap::new(),
            groups: data.groups,
            membership_rule_cache: HashMap::new(),
            group_doc_managers: HashMap::new(),
            groups_pending_bootstrap: HashSet::new(),
            pubsub: PubSubRegistry::new(),
        };

        if let Err(err) = state.rebuild_group_doc_managers() {
            println!(
                "Failed to rebuild group CRDT managers from snapshot: {:?}",
                err
            );
        }

        Ok(state)
    }
}

impl ChatState {
    pub(crate) fn local_group_acl_ready(&self, group_id: &GroupId) -> bool {
        let Some(whitelist) = self.pubsub.whitelist(group_id) else {
            return false;
        };
        let Some(routing) = self.pubsub.routing(group_id) else {
            return false;
        };
        let node = BrokerNodeId::new(our().node.clone());
        let now = SystemTime::now();
        let hub_ok = if routing.hub_topic.is_empty() {
            false
        } else {
            let topic = BrokerTopicId::new(routing.hub_topic.clone());
            whitelist.subscribe_scope(&node, &topic, now).is_some()
                || whitelist.publish_scope(&node, &topic, now).is_some()
        };
        let sub_ok = if routing.subscriber_topic.is_empty() {
            false
        } else {
            let topic = BrokerTopicId::new(routing.subscriber_topic.clone());
            whitelist.subscribe_scope(&node, &topic, now).is_some()
        };
        hub_ok || sub_ok
    }

    #[cfg(feature = "test-helpers")]
    pub fn now_secs() -> u64 {
        Self::now_secs_inner()
    }

    #[cfg(not(feature = "test-helpers"))]
    pub(crate) fn now_secs() -> u64 {
        Self::now_secs_inner()
    }

    fn now_secs_inner() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }

    pub(crate) fn require_hub_access(
        &self,
        group_id: &GroupId,
        node_id: &NodeId,
    ) -> Result<(), String> {
        let group = self
            .groups
            .get(group_id)
            .ok_or_else(|| "group not found".to_string())?;
        let Some(whitelist) = self.pubsub.whitelist(group_id) else {
            return Err("whitelist missing".into());
        };
        if group.routing.hub_topic.is_empty() {
            return Err("hub topic unavailable".into());
        }
        let topic = BrokerTopicId::new(group.routing.hub_topic.clone());
        let node = BrokerNodeId::new(node_id.clone());
        if whitelist
            .publish_scope(&node, &topic, SystemTime::now())
            .is_some()
        {
            Ok(())
        } else {
            Err(format!(
                "node {} lacks publish access for group {}",
                node_id, group_id
            ))
        }
    }

    pub(crate) fn require_hub_subscription(
        &self,
        group_id: &GroupId,
        node_id: &NodeId,
    ) -> Result<(), String> {
        let group = self
            .groups
            .get(group_id)
            .ok_or_else(|| "group not found".to_string())?;
        let Some(whitelist) = self.pubsub.whitelist(group_id) else {
            return Err("whitelist missing".into());
        };
        if group.routing.hub_topic.is_empty() {
            return Err("hub topic unavailable".into());
        }
        let topic = BrokerTopicId::new(group.routing.hub_topic.clone());
        let node = BrokerNodeId::new(node_id.clone());
        if whitelist
            .subscribe_scope(&node, &topic, SystemTime::now())
            .is_some()
        {
            Ok(())
        } else {
            Err(format!(
                "node {} lacks hub subscription for group {}",
                node_id, group_id
            ))
        }
    }

    pub(crate) fn require_subscriber_access(
        &self,
        group_id: &GroupId,
        node_id: &NodeId,
    ) -> Result<(), String> {
        let group = self
            .groups
            .get(group_id)
            .ok_or_else(|| "group not found".to_string())?;
        let Some(whitelist) = self.pubsub.whitelist(group_id) else {
            return Err("whitelist missing".into());
        };
        if group.routing.subscriber_topic.is_empty() {
            return Err("subscriber topic unavailable".into());
        }
        let topic = BrokerTopicId::new(group.routing.subscriber_topic.clone());
        let node = BrokerNodeId::new(node_id.clone());
        if whitelist
            .subscribe_scope(&node, &topic, SystemTime::now())
            .is_some()
        {
            Ok(())
        } else {
            Err(format!(
                "node {} lacks subscribe access for group {}",
                node_id, group_id
            ))
        }
    }

    pub(crate) fn require_group_permission(
        &self,
        group_id: &GroupId,
        node_id: &NodeId,
        permission: u64,
    ) -> Result<(), String> {
        let group = self
            .groups
            .get(group_id)
            .ok_or_else(|| "group not found".to_string())?;
        let member = group
            .members
            .get(node_id)
            .ok_or_else(|| format!("{} is not a member of {}", node_id, group_id))?;
        if member.status != MembershipStatus::Active {
            return Err(format!(
                "member {} is not active in group {}",
                node_id, group_id
            ));
        }
        let role = group.roles.get(&member.role_id).ok_or_else(|| {
            format!(
                "role {} for {} missing in group {}",
                member.role_id, node_id, group_id
            )
        })?;
        if role.permissions.contains(permission) {
            Ok(())
        } else {
            Err(format!(
                "node {} lacks required permission in group {}",
                node_id, group_id
            ))
        }
    }

    pub fn publish_group_delta(&mut self, group_id: &GroupId, update_payload: &str) {
        let local_node = our().node.clone();
        if let Err(err) = self.require_hub_access(group_id, &local_node) {
            println!(
                "[CRDT][{}] skip publish: node {} lacks hub access ({})",
                group_id, local_node, err
            );
            self.replication_metrics.acl_skips =
                self.replication_metrics.acl_skips.saturating_add(1);
            return;
        }
        let (hub_topic, sub_topic) = self
            .groups
            .get(group_id)
            .map(|g| {
                (
                    g.routing.hub_topic.clone(),
                    g.routing.subscriber_topic.clone(),
                )
            })
            .unwrap_or_default();
        let acl_version = self.pubsub.whitelist(group_id).map(|w| w.version());
        if !hub_topic.is_empty() {
            self.publish_broker_message(
                &hub_topic,
                update_payload,
                acl_version,
                ReplicationKind::PushDelta,
            );
        }
        if !sub_topic.is_empty() {
            self.publish_broker_message(
                &sub_topic,
                update_payload,
                acl_version,
                ReplicationKind::PushDelta,
            );
        }
    }

    pub(crate) fn should_seed_group_doc(&self, group: &Group) -> bool {
        group
            .metadata
            .as_ref()
            .map(|meta| meta.creator_id == our().node)
            .unwrap_or(false)
    }

    pub fn group_needs_bootstrap(&self, group_id: &GroupId) -> bool {
        let needs =
            self.groups_pending_bootstrap.contains(group_id) || !self.groups.contains_key(group_id);
        if needs {
            println!(
                "[BOOT] group_needs_bootstrap group_id={} pending_set_contains={} has_group={}",
                group_id,
                self.groups_pending_bootstrap.contains(group_id),
                self.groups.contains_key(group_id)
            );
        }
        needs
    }

    pub(crate) fn refresh_bootstrap_flags(&mut self) {
        let ready: Vec<GroupId> = self
            .groups_pending_bootstrap
            .iter()
            .filter(|gid| {
                let has_local_membership = self
                    .groups
                    .get(*gid)
                    .and_then(|g| g.members.get(&our().node))
                    .map(|m| m.status == MembershipStatus::Active)
                    .unwrap_or(false);
                let acl_ready = self.local_group_acl_ready(gid);
                has_local_membership || acl_ready
            })
            .cloned()
            .collect();
        for gid in ready {
            println!(
                "[BOOT] clearing pending_bootstrap for {} (acl_ready={} local_member_active={})",
                gid,
                self.local_group_acl_ready(&gid),
                self.groups
                    .get(&gid)
                    .and_then(|g| g.members.get(&our().node))
                    .map(|m| m.status == MembershipStatus::Active)
                    .unwrap_or(false)
            );
            self.groups_pending_bootstrap.remove(&gid);
        }
    }

    pub fn mark_group_bootstrapped(&mut self, group_id: &GroupId) {
        println!(
            "[BOOT] mark_group_bootstrapped group_id={} pending_before={}",
            group_id,
            self.groups_pending_bootstrap.contains(group_id)
        );
        self.groups_pending_bootstrap.remove(group_id);
    }

    pub fn ensure_group_doc_manager(
        &mut self,
        group_id: &GroupId,
    ) -> Result<&mut GroupCrdtManager, CommitteeError> {
        if !self.group_doc_managers.contains_key(group_id) {
            // If we don't have the group yet, create an empty doc so the first
            // incoming snapshot can populate it without being merged with a
            // default-initialised state.
            let manager = if let Some(group) = self.groups.get(group_id) {
                GroupCrdtManager::from_group(group_id, group)?
            } else {
                GroupCrdtManager::from_empty(group_id)?
            };
            self.group_doc_managers.insert(group_id.clone(), manager);
        }

        Ok(self
            .group_doc_managers
            .get_mut(group_id)
            .expect("group manager initialised"))
    }

    pub fn groups(&self) -> &HashMap<GroupId, Group> {
        &self.groups
    }

    pub fn groups_mut(&mut self) -> &mut HashMap<GroupId, Group> {
        &mut self.groups
    }

    pub fn group(&self, group_id: &GroupId) -> Option<&Group> {
        self.groups.get(group_id)
    }

    pub fn group_mut(&mut self, group_id: &GroupId) -> Option<&mut Group> {
        self.groups.get_mut(group_id)
    }

    pub fn upsert_group(&mut self, group_id: GroupId, group: Group) -> Option<Group> {
        self.groups.insert(group_id, group)
    }

    pub fn remove_group(&mut self, group_id: &GroupId) -> Option<Group> {
        let removed = self.groups.remove(group_id);
        self.group_doc_managers.remove(group_id);
        self.groups_pending_bootstrap.remove(group_id);
        self.membership_rule_cache.remove(group_id);
        self.pubsub.remove_group(group_id);
        removed
    }

    pub fn create_group_state(
        &mut self,
        mut req: CreateGroupReq,
    ) -> Result<CreateGroupRes, String> {
        let group_id = req.group_id.take().unwrap_or_else(generate_group_id);
        if self.groups.contains_key(&group_id) {
            return Err("Group already exists".to_string());
        }

        let now = current_timestamp();
        let creator = our().node.clone();
        let mut counters = GroupCounters::default();
        let root_thread_id = counters.next_thread_id(&group_id);

        let default_role_id = format!("{group_id}:member");
        let owner_role_id = format!("{group_id}:owner");
        let visibility = req.visibility.unwrap_or(GroupVisibility::Private);

        let metadata = GroupMetadata::new(
            req.name,
            req.description,
            req.avatar,
            creator.clone(),
            now,
            now,
            visibility,
            default_role_id.clone(),
            root_thread_id.clone(),
        );

        let mut group = Group::new(metadata);
        group.routing = GroupRoutingConfig::for_group(&group_id);
        group.counters = counters;

        let owner_role = Role::new(
            owner_role_id.clone(),
            "Owner",
            GroupPermissions::all(),
            GroupTier::Hub,
        );
        let mut member_permissions = GroupPermissions::empty();
        member_permissions.insert(GroupPermissions::SEND_MESSAGES);
        member_permissions.insert(GroupPermissions::CREATE_THREADS);
        let member_role = Role::new(
            default_role_id.clone(),
            req.default_role_label
                .unwrap_or_else(|| "Member".to_string()),
            member_permissions,
            GroupTier::Subscriber,
        );

        group.roles.insert(owner_role_id.clone(), owner_role);
        group.roles.insert(default_role_id.clone(), member_role);

        group.members.insert(
            creator.clone(),
            GroupMember::new(
                creator.clone(),
                owner_role_id,
                MembershipStatus::Active,
                now,
            ),
        );
        group.hubs.active.insert(creator.clone());
        group.hubs.upsert_sync(
            creator.clone(),
            HubSyncState {
                last_seen_ts: now,
                ..HubSyncState::default()
            },
        );
        group.subscribers.entries.insert(
            creator.clone(),
            SubscriberSyncState {
                last_state_vector: None,
                last_snapshot_digest: None,
                last_seen_ts: now,
            },
        );
        group.delivery.hub_cursors.insert(
            creator.clone(),
            DeliveryCursor {
                queue_id: group.routing.hub_topic.clone(),
                last_offset: 0,
                updated_at: now,
            },
        );
        group.delivery.subscriber_cursors.insert(
            creator.clone(),
            DeliveryCursor {
                queue_id: group.routing.subscriber_topic.clone(),
                last_offset: 0,
                updated_at: now,
            },
        );

        group.membership_rules = ensure_membership_rules(req.membership_rules, &creator);

        let mut root_thread = Thread::new(
            root_thread_id.clone(),
            group_id.clone(),
            0,
            ThreadParentRef::Root(group_id.clone()),
            now,
            creator.clone(),
        );
        root_thread.title = req.root_thread_title;
        root_thread.summary.last_activity = now;
        root_thread.summary.last_sender = Some(creator);
        group.threads.insert(root_thread_id, root_thread);

        self.groups.insert(group_id.clone(), group);
        self.mark_group_bootstrapped(&group_id);
        self.commit_group_crdt_or_log(&group_id, "create_group");

        Ok(CreateGroupRes { group_id })
    }

    pub fn list_groups_state(&self) -> ListGroupsRes {
        let groups = self
            .groups
            .iter()
            .map(|(group_id, group)| GroupSummary {
                group_id: group_id.clone(),
                metadata: group.metadata.clone(),
                member_count: group.members.len(),
                thread_count: group.threads.len(),
            })
            .collect();
        ListGroupsRes { groups }
    }

    pub fn get_group_state(&self, req: GetGroupReq) -> GetGroupRes {
        GetGroupRes {
            group: self.groups.get(&req.group_id).cloned(),
        }
    }

    pub fn create_group_thread_state(
        &mut self,
        mut req: CreateGroupThreadReq,
    ) -> Result<CreateGroupThreadRes, String> {
        self.require_group_permission(&req.group_id, &our().node, GroupPermissions::CREATE_THREADS)
            .map_err(|err| format!("cannot create thread: {}", err))?;
        self.require_subscriber_access(&req.group_id, &our().node)
            .map_err(|err| format!("cannot create thread: {}", err))?;

        let thread_id = self.next_group_thread_id(&req.group_id)?;
        let now = current_timestamp();
        let creator = our().node.clone();

        {
            let group = self
                .groups
                .get_mut(&req.group_id)
                .ok_or_else(|| "Group not found".to_string())?;

            let root_thread_id = group_root_thread_id(group)
                .ok_or_else(|| "Group missing root thread".to_string())?;

            let (parent_ref, depth, parent_child) =
                if let Some(parent_id) = req.parent_thread_id.take() {
                    let parent = group
                        .threads
                        .get(&parent_id)
                        .ok_or_else(|| "Parent thread not found".to_string())?;
                    (
                        ThreadParentRef::Thread(parent_id.clone()),
                        parent.depth + 1,
                        Some(parent_id.clone()),
                    )
                } else {
                    (
                        ThreadParentRef::Root(root_thread_id.clone()),
                        0,
                        Some(root_thread_id.clone()),
                    )
                };

            let mut thread = Thread::new(
                thread_id.clone(),
                req.group_id.clone(),
                depth,
                parent_ref,
                now,
                creator,
            );
            thread.title = req.title.take();
            thread.summary.last_activity = now;

            group.threads.insert(thread_id.clone(), thread);
            if let Some(parent_id) = parent_child {
                if let Some(parent) = group.threads.get_mut(&parent_id) {
                    parent.child_threads.push(thread_id.clone());
                }
            }
            if let Some(meta) = group.metadata.as_mut() {
                // Ensure updated_at advances even if called within the same second.
                let mut ts = now;
                if meta.updated_at >= ts {
                    ts = meta.updated_at.saturating_add(1);
                }
                meta.updated_at = ts;
            }
        }
        self.commit_group_crdt_or_log(&req.group_id, "create_group_thread");
        Ok(CreateGroupThreadRes { thread_id })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_state_runtime_fields_are_not_serialized() {
        let mut state = ChatState::default();
        state.profile.name = "alice".to_string();
        state.ws_connections.insert(1, "peer".to_string());
        state.browser_connections.insert("browser".to_string(), 2);
        state.last_heartbeat.insert(3, 123);
        state.active_connections.insert(4);
        state.node_profiles.insert(
            "peer".to_string(),
            UserProfile {
                name: "bob".into(),
                profile_pic: None,
            },
        );

        let value = serde_json::to_value(&state).expect("serialize ChatState");

        assert!(value.get("ws_connections").is_none());
        assert!(value.get("browser_connections").is_none());
        assert!(value.get("last_heartbeat").is_none());
        assert!(value.get("active_connections").is_none());
        assert!(value.get("node_profiles").is_none());

        let restored: ChatState = serde_json::from_value(value).expect("deserialize ChatState");

        assert_eq!(restored.profile.name, "alice");
        assert!(restored.ws_connections.is_empty());
        assert!(restored.browser_connections.is_empty());
        assert!(restored.last_heartbeat.is_empty());
        assert!(restored.active_connections.is_empty());
        assert!(restored.node_profiles.is_empty());
    }
}
