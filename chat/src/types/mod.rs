use futures::channel::mpsc::{self, UnboundedReceiver, UnboundedSender};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{hash_map::DefaultHasher, HashMap, HashSet, VecDeque};
use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::crdt::{
    compile_membership_rules, AttachmentDescriptor, DeliveryCursor, Group, GroupCounters,
    GroupCrdtManager, GroupDocState, GroupId, GroupMember, GroupMetadata, GroupPermissions,
    GroupRoutingConfig, GroupTier, GroupVisibility, HubSyncState, MembershipActionKind,
    MembershipDecision, MembershipDecisionStatus, MembershipProposal, MembershipRuleBox,
    MembershipRuleConfig, MembershipRuleError, MembershipStatus, MessageId, MessageMeta, NodeId,
    Role, SubscriberSyncState, Thread, ThreadId, ThreadParentRef,
};
use crate::log_crdt_event;
use crate::pubsub::PubSubRegistry;
use hyperware_crdt::{
    yrs::{Decode, Encode, StateVector},
    CommitteeError,
};
use hyperware_process_lib::our;
use hyperware_pubsub_core::{whitelist::NodeId as BrokerNodeId, TopicId as BrokerTopicId};

const SUBSCRIBER_LANE_TTL_SECS: u64 = 300;
const SUBSCRIBER_ACK_DEADLINE_SECS: u64 = 45;
const DELIVERY_DEDUPE_WINDOW_SECS: u64 = 120;
const DELIVERY_DEDUPE_LIMIT: usize = 2048;
const SUBSCRIBER_EVENT_BUFFER: usize = 256;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PushSubscription {
    pub endpoint: String,
    pub keys: SubscriptionKeys,
}

fn aggregate_rule_decisions(
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

fn active_member_count(group: &Group) -> u32 {
    group
        .members
        .values()
        .filter(|member| member.status == MembershipStatus::Active)
        .count() as u32
}

fn membership_proposal_key(
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

fn current_timestamp() -> u64 {
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

fn group_root_thread_id(group: &Group) -> Option<ThreadId> {
    group
        .metadata
        .as_ref()
        .map(|metadata| metadata.root_thread_id.clone())
}

fn sync_member_membership_sets(group: &mut Group, member_id: &NodeId, timestamp: u64) {
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
    pub online_nodes: HashSet<String>,
    pub ws_connections: HashMap<u32, String>,
    pub browser_connections: HashMap<String, u32>,
    pub last_heartbeat: HashMap<u32, u64>,
    #[serde(default)]
    pub active_connections: HashSet<u32>,
    #[serde(default)]
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
            online_nodes: HashSet::new(),
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
            online_nodes: HashSet<String>,
            ws_connections: HashMap<u32, String>,
            browser_connections: HashMap<String, u32>,
            last_heartbeat: HashMap<u32, u64>,
            #[serde(default)]
            active_connections: HashSet<u32>,
            #[serde(default)]
            node_profiles: HashMap<String, UserProfile>,
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
            online_nodes: data.online_nodes,
            ws_connections: data.ws_connections,
            browser_connections: data.browser_connections,
            last_heartbeat: data.last_heartbeat,
            active_connections: data.active_connections,
            node_profiles: data.node_profiles,
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

    fn dedupe_key(topic: &str, payload: &str) -> u64 {
        let mut hasher = DefaultHasher::new();
        topic.hash(&mut hasher);
        payload.hash(&mut hasher);
        hasher.finish()
    }

    fn register_delivery_fingerprint(&mut self, topic: &str, payload: &str, now: u64) -> bool {
        let key = Self::dedupe_key(topic, payload);
        if let Some(ts) = self.delivery_dedupe.get(&key) {
            if now.saturating_sub(*ts) < DELIVERY_DEDUPE_WINDOW_SECS {
                return true;
            }
        }
        self.delivery_dedupe.insert(key, now);
        self.prune_dedupe_cache(now);
        false
    }

    fn prune_dedupe_cache(&mut self, now: u64) {
        let cutoff = now.saturating_sub(DELIVERY_DEDUPE_WINDOW_SECS);
        self.delivery_dedupe.retain(|_, ts| *ts >= cutoff);
        if self.delivery_dedupe.len() > DELIVERY_DEDUPE_LIMIT {
            let overflow = self
                .delivery_dedupe
                .len()
                .saturating_sub(DELIVERY_DEDUPE_LIMIT);
            if overflow == 0 {
                return;
            }
            let mut oldest: Vec<(u64, u64)> = self
                .delivery_dedupe
                .iter()
                .map(|(k, ts)| (*k, *ts))
                .collect();
            oldest.sort_by_key(|(_, ts)| *ts);
            for (key, _) in oldest.into_iter().take(overflow) {
                self.delivery_dedupe.remove(&key);
            }
        }
    }

    fn record_subscriber_event(&mut self, event: SubscriberDeliveryEvent) {
        self.subscriber_events.push_back(event);
        if self.subscriber_events.len() > SUBSCRIBER_EVENT_BUFFER {
            let overflow = self.subscriber_events.len() - SUBSCRIBER_EVENT_BUFFER;
            for _ in 0..overflow {
                self.subscriber_events.pop_front();
            }
        }
    }

    pub fn rebuild_group_doc_managers(&mut self) -> Result<(), CommitteeError> {
        self.ensure_routing_defaults_for_all();
        self.group_doc_managers.clear();
        self.groups_pending_bootstrap.clear();
        for (group_id, group) in &self.groups {
            if self.should_seed_group_doc(group) {
                let manager = GroupCrdtManager::from_group(group_id, group)?;
                self.group_doc_managers.insert(group_id.clone(), manager);
            } else {
                self.groups_pending_bootstrap.insert(group_id.clone());
            }
        }
        self.pubsub.rebuild_all(&self.groups);
        Ok(())
    }

    pub(crate) fn enqueue_replication_task(&mut self, task: ReplicationTask) {
        self.replication_queue.push_back(task);
        self.wake_replication_worker();
    }

    pub(crate) fn next_ready_replication_task(&mut self, now: u64) -> Option<ReplicationTask> {
        let mut rotate = 0usize;
        while let Some(task) = self.replication_queue.pop_front() {
            if task.not_before <= now {
                return Some(task);
            }
            self.replication_queue.push_back(task);
            rotate += 1;
            if rotate >= self.replication_queue.len() {
                break;
            }
        }
        None
    }

    pub(crate) fn has_replication_task(
        &self,
        group_id: &GroupId,
        peer: &str,
        kind: ReplicationKind,
    ) -> bool {
        self.replication_queue.iter().any(|t| {
            &t.group_id == group_id
                && t.peer == peer
                && std::mem::discriminant(&t.kind) == std::mem::discriminant(&kind)
        })
    }

    pub fn wake_replication_worker(&self) {
        if let Some(tx) = &self.replication_wake_tx {
            tx.wake();
        }
    }

    pub fn rebuild_pubsub_for_group(&mut self, group_id: &GroupId) {
        if let Some(group) = self.groups.get(group_id) {
            self.pubsub.rebuild_group(group_id, group);
        } else {
            self.pubsub.remove_group(group_id);
        }
    }

    fn ensure_routing_defaults_for_all(&mut self) {
        for (group_id, group) in self.groups.iter_mut() {
            if group.routing.hub_topic.is_empty() || group.routing.subscriber_topic.is_empty() {
                group.routing = GroupRoutingConfig::for_group(group_id);
            }
        }
    }

    pub(crate) fn peer_state_vector(&self, group_id: &GroupId, peer: &str) -> Option<StateVector> {
        let group = self.groups.get(group_id)?;
        let sync = group.hubs.sync.get(peer)?;
        let bytes = sync.last_state_vector.as_ref()?;
        StateVector::decode_v1(bytes).ok()
    }

    pub(crate) fn update_peer_state_vector(
        &mut self,
        group_id: &GroupId,
        peer: &str,
        sv: &StateVector,
    ) {
        if let Some(group) = self.groups.get_mut(group_id) {
            let now = Self::now_secs();
            group.hubs.upsert_sync(
                peer.to_string(),
                HubSyncState {
                    last_state_vector: Some(sv.encode_v1()),
                    last_seen_ts: now,
                    ..HubSyncState::default()
                },
            );
        }
    }

    pub(crate) fn update_local_hub_sync_state(&mut self, group_id: &GroupId, sv: &StateVector) {
        if let Some(group) = self.groups.get_mut(group_id) {
            let now = Self::now_secs();
            group.hubs.upsert_sync(
                our().node.clone(),
                HubSyncState {
                    last_state_vector: Some(sv.encode_v1()),
                    last_seen_ts: now,
                    ..HubSyncState::default()
                },
            );
        }
    }

    pub(crate) fn update_delivery_cursor(
        &mut self,
        group_id: &GroupId,
        peer: &str,
        is_hub: bool,
        queue_id: String,
        offset: Option<u64>,
    ) {
        if let Some(group) = self.groups.get_mut(group_id) {
            let now = Self::now_secs();
            let cursors = if is_hub {
                &mut group.delivery.hub_cursors
            } else {
                &mut group.delivery.subscriber_cursors
            };
            let entry = cursors
                .entry(peer.to_string())
                .or_insert_with(|| DeliveryCursor {
                    queue_id: queue_id.clone(),
                    last_offset: 0,
                    updated_at: now,
                });
            entry.queue_id = queue_id;
            let next = offset.unwrap_or_else(|| entry.last_offset.saturating_add(1));
            entry.last_offset = next;
            entry.updated_at = now;
        }
    }

    pub(crate) fn enqueue_replication_pushes(
        &mut self,
        group_id: &GroupId,
        state_vector_bytes: Vec<u8>,
    ) {
        let now = Self::now_secs();
        let Some(group) = self.groups.get(group_id).cloned() else {
            return;
        };
        // Hubs
        for hub in &group.hubs.active {
            if hub == &our().node {
                continue;
            }
            let since = self
                .peer_state_vector(group_id, hub)
                .map(|sv| sv.encode_v1());
            let hub_age = group
                .delivery
                .hub_cursors
                .get(hub)
                .map(|c| now.saturating_sub(c.updated_at))
                .unwrap_or(u64::MAX);
            let kind = if since.is_none() || hub_age > SUBSCRIBER_LANE_TTL_SECS {
                ReplicationKind::PushSnapshot
            } else {
                ReplicationKind::PushDelta
            };
            let task = ReplicationTask {
                group_id: group_id.clone(),
                peer: hub.clone(),
                kind,
                since,
                attempt: 0,
                not_before: now,
            };
            self.enqueue_replication_task(task);
        }

        // Subscribers
        for (node_id, member) in &group.members {
            if node_id == &our().node {
                continue;
            }
            if let Some(role) = group.roles.get(&member.role_id) {
                if role.tier == GroupTier::Hub {
                    continue;
                }
            }
            let since = self
                .peer_state_vector(group_id, node_id)
                .map(|sv| sv.encode_v1());
            let cursor_age = group
                .delivery
                .subscriber_cursors
                .get(node_id)
                .map(|c| now.saturating_sub(c.updated_at))
                .unwrap_or(u64::MAX);
            let kind = if since.is_none() || cursor_age > SUBSCRIBER_LANE_TTL_SECS {
                ReplicationKind::PushSnapshot
            } else {
                ReplicationKind::PushDelta
            };
            self.enqueue_replication_task(ReplicationTask {
                group_id: group_id.clone(),
                peer: node_id.clone(),
                kind,
                since,
                attempt: 0,
                not_before: now,
            });
        }

        // Ensure we have our own sync recorded
        if let Ok(sv) = StateVector::decode_v1(&state_vector_bytes) {
            self.update_local_hub_sync_state(group_id, &sv);
        }
    }

    pub(crate) fn publish_broker_message(
        &mut self,
        topic: &str,
        payload: &str,
        acl_version: Option<u64>,
        kind: ReplicationKind,
    ) {
        let next = *self.broker_offsets.get(topic).unwrap_or(&0);
        let env = BrokerEnvelope {
            offset: next,
            payload: payload.to_string(),
            acl_version,
            kind,
            ts: Self::now_secs(),
        };
        let entry = self
            .broker_queues
            .entry(topic.to_string())
            .or_insert_with(VecDeque::new);
        entry.push_back(env);
        self.broker_offsets
            .insert(topic.to_string(), next.saturating_add(1));
        println!(
            "[BROKER] topic={} enqueued offset={} len={}",
            topic,
            next,
            entry.len()
        );
        self.wake_replication_worker();
    }

    pub(crate) fn consume_broker_topics(&mut self, max_per_topic: usize) -> usize {
        let mut applied = 0usize;
        let now = Self::now_secs();
        let topics: Vec<String> = self
            .groups
            .values()
            .flat_map(|g| {
                let mut t = Vec::new();
                if g.hubs.active.contains(&our().node) && !g.routing.hub_topic.is_empty() {
                    t.push(g.routing.hub_topic.clone());
                }
                // subscriber lane consumption if we are in subscribers
                if g.subscribers.entries.contains_key(&our().node)
                    && !g.routing.subscriber_topic.is_empty()
                {
                    t.push(g.routing.subscriber_topic.clone());
                }
                t
            })
            .collect();

        for topic in topics {
            let from = *self.broker_cursors.get(&topic).unwrap_or(&0);
            let envelopes: Vec<BrokerEnvelope> = self
                .broker_queues
                .get(&topic)
                .map(|q| {
                    q.iter()
                        .filter(|e| e.offset >= from)
                        .take(max_per_topic)
                        .cloned()
                        .collect()
                })
                .unwrap_or_default();

            for env in envelopes {
                if let Err(err) = self.apply_broker_envelope(&topic, &env, now) {
                    println!(
                        "[BROKER] topic={} offset={} apply error: {}",
                        topic, env.offset, err
                    );
                    continue;
                }
                applied += 1;
                self.broker_cursors.insert(topic.clone(), env.offset + 1);
            }
        }
        applied
    }

    #[cfg(feature = "test-helpers")]
    pub fn enqueue_stale_subscriber_replays(&mut self, now: u64) {
        self.enqueue_stale_subscriber_replays_inner(now);
    }

    #[cfg(not(feature = "test-helpers"))]
    pub(crate) fn enqueue_stale_subscriber_replays(&mut self, now: u64) {
        self.enqueue_stale_subscriber_replays_inner(now);
    }

    fn enqueue_stale_subscriber_replays_inner(&mut self, now: u64) {
        let groups: Vec<(GroupId, Group)> = self
            .groups
            .iter()
            .map(|(id, group)| (id.clone(), group.clone()))
            .collect();
        for (group_id, group) in groups {
            if group.routing.subscriber_topic.is_empty() {
                continue;
            }
            for (node_id, _) in group.subscribers.entries.iter() {
                if node_id == &our().node {
                    continue;
                }
                if self.has_replication_task(&group_id, node_id, ReplicationKind::PushSnapshot)
                    || self.has_replication_task(&group_id, node_id, ReplicationKind::PushDelta)
                {
                    continue;
                }
                let cursor_age = group
                    .delivery
                    .subscriber_cursors
                    .get(node_id)
                    .map(|cursor| now.saturating_sub(cursor.updated_at));
                let stale = cursor_age
                    .map(|age| age > SUBSCRIBER_ACK_DEADLINE_SECS)
                    .unwrap_or(true);
                if !stale {
                    continue;
                }
                let since = self
                    .peer_state_vector(&group_id, node_id)
                    .map(|sv| sv.encode_v1());
                let age = cursor_age.unwrap_or(0);
                let kind = if since.is_none() || age > SUBSCRIBER_LANE_TTL_SECS {
                    ReplicationKind::PushSnapshot
                } else {
                    ReplicationKind::PushDelta
                };
                let kind_for_log = kind.clone();
                self.enqueue_replication_task(ReplicationTask {
                    group_id: group_id.clone(),
                    peer: node_id.clone(),
                    kind,
                    since,
                    attempt: 0,
                    not_before: now,
                });
                self.replication_metrics.stale_replays =
                    self.replication_metrics.stale_replays.saturating_add(1);
                println!(
                    "[REPL][{}] queued {} replay to subscriber {} (age={}s)",
                    group_id,
                    match kind_for_log {
                        ReplicationKind::PushSnapshot => "snapshot",
                        _ => "delta",
                    },
                    node_id,
                    age
                );
            }
        }
    }

    #[cfg(feature = "test-helpers")]
    pub fn apply_broker_envelope(
        &mut self,
        topic: &str,
        env: &BrokerEnvelope,
        now: u64,
    ) -> Result<(), String> {
        self.apply_broker_envelope_inner(topic, env, now)
    }

    #[cfg(not(feature = "test-helpers"))]
    fn apply_broker_envelope(
        &mut self,
        topic: &str,
        env: &BrokerEnvelope,
        now: u64,
    ) -> Result<(), String> {
        self.apply_broker_envelope_inner(topic, env, now)
    }

    fn apply_broker_envelope_inner(
        &mut self,
        topic: &str,
        env: &BrokerEnvelope,
        now: u64,
    ) -> Result<(), String> {
        // find group by topic
        let group_id = self
            .groups
            .iter()
            .find(|(_, g)| g.routing.hub_topic == topic || g.routing.subscriber_topic == topic)
            .map(|(id, _)| id.clone())
            .ok_or_else(|| "no group for topic".to_string())?;
        let is_subscriber_topic = self
            .groups
            .get(&group_id)
            .map(|g| g.routing.subscriber_topic == topic)
            .unwrap_or(false);
        let created_at = if env.ts == 0 { now } else { env.ts };
        let age = now.saturating_sub(created_at);
        if age > SUBSCRIBER_ACK_DEADLINE_SECS {
            println!(
                "[BROKER][{}] delivery lag {}s topic={} offset={}",
                group_id, age, topic, env.offset
            );
        }
        if is_subscriber_topic {
            self.replication_metrics.last_subscriber_lag_secs = age;
            if age > SUBSCRIBER_LANE_TTL_SECS {
                self.replication_metrics.drops = self.replication_metrics.drops.saturating_add(1);
                println!(
                    "[BROKER][{}] drop stale subscriber envelope topic={} offset={} age={}s",
                    group_id, topic, env.offset, age
                );
                return Ok(());
            }
            if self.register_delivery_fingerprint(topic, &env.payload, now) {
                self.replication_metrics.drops = self.replication_metrics.drops.saturating_add(1);
                println!(
                    "[BROKER][{}] drop duplicate subscriber envelope topic={} offset={}",
                    group_id, topic, env.offset
                );
                return Ok(());
            }
        } else {
            self.replication_metrics.last_lag_secs = age;
        }

        // ACL drift log
        if let Some(in_acl) = env.acl_version {
            if let Some(wl) = self.pubsub.whitelist(&group_id) {
                let local = wl.version();
                if local != in_acl {
                    println!(
                        "[BROKER][{}] ACL drift topic {} incoming={} local={}",
                        group_id, topic, in_acl, local
                    );
                    self.replication_metrics.acl_drifts =
                        self.replication_metrics.acl_drifts.saturating_add(1);
                }
            }
        }

        self.apply_group_update_payload(
            &group_id,
            &env.payload,
            "broker_delivery",
            env.acl_version,
            is_subscriber_topic,
        )
        .map_err(|err| {
            self.replication_metrics.drops = self.replication_metrics.drops.saturating_add(1);
            err
        })?;
        // update cursors/delivery trackers
        let is_hub = self
            .groups
            .get(&group_id)
            .map(|g| g.routing.hub_topic == topic)
            .unwrap_or(false);
        self.update_delivery_cursor(
            &group_id,
            &our().node,
            is_hub,
            topic.to_string(),
            Some(env.offset),
        );
        // bump heartbeat
        if let Some(group) = self.groups.get_mut(&group_id) {
            group.hubs.upsert_sync(
                our().node.clone(),
                HubSyncState {
                    last_seen_ts: now,
                    ..HubSyncState::default()
                },
            );
            if is_subscriber_topic {
                let subscriber = group
                    .subscribers
                    .entries
                    .entry(our().node.clone())
                    .or_insert_with(SubscriberSyncState::default);
                subscriber.last_seen_ts = now;
                subscriber.last_state_vector = self
                    .group_doc_managers
                    .get(&group_id)
                    .and_then(|mgr| mgr.last_state_vector().map(|sv| sv.encode_v1()));
                if let ReplicationKind::PushSnapshot = env.kind {
                    let digest = format!("{:x}", Self::dedupe_key(topic, &env.payload));
                    subscriber.last_snapshot_digest = Some(digest);
                }
            }
        }
        if is_subscriber_topic {
            self.record_subscriber_event(SubscriberDeliveryEvent {
                group_id,
                topic: topic.to_string(),
                offset: env.offset,
                kind: env.kind.clone(),
                age_secs: age,
                recorded_at: now,
            });
        }
        Ok(())
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

    fn require_group_permission(
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

    fn should_seed_group_doc(&self, group: &Group) -> bool {
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

    pub fn commit_group_crdt(&mut self, group_id: &GroupId) -> Result<(), CommitteeError> {
        if self.group_needs_bootstrap(group_id) {
            return Err(CommitteeError::Observer(format!(
                "group {} requires bootstrap before CRDT commit",
                group_id
            )));
        }

        let group = self.groups.get(group_id).ok_or_else(|| {
            CommitteeError::Observer(format!("missing group {} for CRDT commit", group_id))
        })?;
        self.pubsub.rebuild_group(group_id, group);
        let snapshot: GroupDocState = (group_id, group).into();
        let manager = self.ensure_group_doc_manager(group_id)?;
        manager.refresh_with_snapshot(snapshot)?;
        let state_vector = {
            let doc = manager.doc();
            let state_vector = doc.state_vector();
            log_crdt_event(doc.id(), "commit_group_crdt", &state_vector, None);
            state_vector
        };
        manager.set_last_state_vector(state_vector.clone());
        self.update_local_hub_sync_state(group_id, &state_vector);
        // enqueue per-peer fanout for hubs and subscribers
        self.enqueue_replication_pushes(group_id, state_vector.encode_v1());
        Ok(())
    }

    pub fn commit_group_crdt_or_log(&mut self, group_id: &GroupId, context: &str) {
        if let Err(err) = self.commit_group_crdt(group_id) {
            println!(
                "Failed to commit group CRDT state (group={} context={}): {:?}",
                group_id, context, err
            );
        }
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

    fn next_group_thread_id(&mut self, group_id: &GroupId) -> Result<ThreadId, String> {
        let group = self
            .groups
            .get_mut(group_id)
            .ok_or_else(|| "Group not found".to_string())?;
        Ok(group.counters.next_thread_id(group_id))
    }

    fn next_group_message_id(&mut self, group_id: &GroupId) -> Result<MessageId, String> {
        let group = self
            .groups
            .get_mut(group_id)
            .ok_or_else(|| "Group not found".to_string())?;
        Ok(group.counters.next_message_id(group_id))
    }

    pub fn set_group_membership_rules(
        &mut self,
        group_id: GroupId,
        rules: Vec<MembershipRuleConfig>,
    ) {
        let entry = self
            .groups
            .entry(group_id.clone())
            .or_insert_with(Group::default);
        entry.membership_rules = rules;
        self.invalidate_group_rules(&group_id);
        self.commit_group_crdt_or_log(&group_id, "set_group_membership_rules");
    }

    pub fn group_rules(
        &mut self,
        group_id: &GroupId,
    ) -> Result<&[MembershipRuleBox], MembershipRuleError> {
        if !self.membership_rule_cache.contains_key(group_id) {
            self.rebuild_group_rule_cache(group_id)?;
        }

        Ok(self
            .membership_rule_cache
            .get(group_id)
            .expect("group rules cache populated after rebuild")
            .as_slice())
    }

    pub fn invalidate_group_rules(&mut self, group_id: &GroupId) {
        self.membership_rule_cache.remove(group_id);
    }

    pub fn rebuild_group_rule_cache(
        &mut self,
        group_id: &GroupId,
    ) -> Result<(), MembershipRuleError> {
        let configs = self
            .groups
            .get(group_id)
            .map(|group| group.membership_rules.as_slice())
            .unwrap_or(&[]);
        let compiled = compile_membership_rules(configs)?;
        self.membership_rule_cache
            .insert(group_id.clone(), compiled);
        Ok(())
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

    pub fn send_group_message_state(
        &mut self,
        mut req: SendGroupMessageReq,
    ) -> Result<SendGroupMessageRes, String> {
        self.require_group_permission(&req.group_id, &our().node, GroupPermissions::SEND_MESSAGES)
            .map_err(|err| format!("cannot send group message: {}", err))?;
        self.require_subscriber_access(&req.group_id, &our().node)
            .map_err(|err| format!("cannot send group message: {}", err))?;

        let message_id = self.next_group_message_id(&req.group_id)?;
        let now = current_timestamp();
        let sender = our().node.clone();

        let message = {
            let group = self
                .groups
                .get_mut(&req.group_id)
                .ok_or_else(|| "Group not found".to_string())?;

            let thread_id = req
                .thread_id
                .take()
                .or_else(|| group_root_thread_id(group))
                .ok_or_else(|| "Group missing root thread".to_string())?;

            let thread = group
                .threads
                .get_mut(&thread_id)
                .ok_or_else(|| "Thread not found".to_string())?;

            let mut message = MessageMeta::new(
                message_id.clone(),
                thread_id.clone(),
                req.group_id.clone(),
                sender.clone(),
                now,
                req.message_type,
            );
            message.reply_to = req.reply_to.take();
            message.attachments = req.attachments.clone();

            if thread.root_message_id.is_none() {
                thread.root_message_id = Some(message_id.clone());
            }
            thread.summary.message_count += 1;
            thread.summary.last_message_id = Some(message_id.clone());
            thread.summary.last_activity = now;
            thread.summary.last_sender = Some(sender.clone());

            group.messages.insert(message_id.clone(), message.clone());
            if let Some(meta) = group.metadata.as_mut() {
                meta.updated_at = now;
            }

            let subscriber = group
                .subscribers
                .entries
                .entry(sender.clone())
                .or_insert_with(SubscriberSyncState::default);
            subscriber.last_seen_ts = now;

            message
        };
        self.commit_group_crdt_or_log(&req.group_id, "send_group_message");
        Ok(SendGroupMessageRes { message })
    }

    pub fn invite_member(
        &mut self,
        group_id: &GroupId,
        proposer: NodeId,
        candidate: NodeId,
        role_id: String,
    ) -> Result<MembershipDecision, MembershipActionError> {
        self.require_group_permission(group_id, &proposer, GroupPermissions::INVITE_MEMBERS)
            .map_err(MembershipActionError::PermissionDenied)?;

        let proposal_id =
            membership_proposal_key(group_id, &candidate, MembershipActionKind::Invite);
        let eligible_voters = {
            let group = self
                .groups
                .get(group_id)
                .ok_or_else(|| MembershipActionError::GroupNotFound(group_id.clone()))?;
            if let Some(member) = group.members.get(&candidate) {
                if member.status != MembershipStatus::Removed {
                    return Err(MembershipActionError::MemberExists(candidate));
                }
            }
            if group.membership_proposals.contains_key(&proposal_id) {
                return Err(MembershipActionError::ProposalExists(proposal_id));
            }
            active_member_count(group)
        };

        let mut proposal = MembershipProposal {
            proposal_id,
            candidate,
            requested_role: role_id,
            proposer,
            action: MembershipActionKind::Invite,
            approvals: HashSet::new(),
            rejections: HashSet::new(),
            eligible_voters,
            token_support: 0,
            token_opposition: 0,
        };
        proposal.approvals.insert(proposal.proposer.clone());
        self.process_membership_proposal(group_id, proposal)
    }

    pub fn approve_membership(
        &mut self,
        group_id: &GroupId,
        proposal_id: &str,
        approver: NodeId,
    ) -> Result<MembershipDecision, MembershipActionError> {
        self.require_group_permission(group_id, &approver, GroupPermissions::INVITE_MEMBERS)
            .map_err(MembershipActionError::PermissionDenied)?;

        let proposal = {
            let group = self
                .groups
                .get_mut(group_id)
                .ok_or_else(|| MembershipActionError::GroupNotFound(group_id.clone()))?;
            let proposal = group
                .membership_proposals
                .get_mut(proposal_id)
                .ok_or_else(|| MembershipActionError::ProposalNotFound(proposal_id.to_string()))?;
            proposal.approvals.insert(approver);
            proposal.clone()
        };
        self.process_membership_proposal(group_id, proposal)
    }

    pub fn remove_member(
        &mut self,
        group_id: &GroupId,
        proposer: NodeId,
        target: NodeId,
    ) -> Result<MembershipDecision, MembershipActionError> {
        self.require_group_permission(group_id, &proposer, GroupPermissions::MANAGE_ROLES)
            .map_err(MembershipActionError::PermissionDenied)?;

        let proposal_id = membership_proposal_key(group_id, &target, MembershipActionKind::Remove);
        let (eligible_voters, role_id) = {
            let group = self
                .groups
                .get(group_id)
                .ok_or_else(|| MembershipActionError::GroupNotFound(group_id.clone()))?;
            let member = group
                .members
                .get(&target)
                .ok_or_else(|| MembershipActionError::MemberNotFound(target.clone()))?;
            if member.status == MembershipStatus::Removed {
                return Err(MembershipActionError::MemberNotFound(target));
            }
            if group.membership_proposals.contains_key(&proposal_id) {
                return Err(MembershipActionError::ProposalExists(proposal_id));
            }
            (active_member_count(group), member.role_id.clone())
        };

        let mut proposal = MembershipProposal {
            proposal_id,
            candidate: target,
            requested_role: role_id,
            proposer,
            action: MembershipActionKind::Remove,
            approvals: HashSet::new(),
            rejections: HashSet::new(),
            eligible_voters,
            token_support: 0,
            token_opposition: 0,
        };
        proposal.approvals.insert(proposal.proposer.clone());
        self.process_membership_proposal(group_id, proposal)
    }

    fn evaluate_membership(
        &mut self,
        group_id: &GroupId,
        proposal: &MembershipProposal,
    ) -> Result<MembershipDecision, MembershipActionError> {
        let rules = self.group_rules(group_id)?;
        Ok(aggregate_rule_decisions(rules, proposal))
    }

    fn apply_membership_decision(
        &mut self,
        group_id: &GroupId,
        proposal: MembershipProposal,
        decision: &MembershipDecision,
        now: u64,
    ) -> Result<(), MembershipActionError> {
        let group = self
            .groups
            .get_mut(group_id)
            .ok_or_else(|| MembershipActionError::GroupNotFound(group_id.clone()))?;

        if let Some(meta) = group.metadata.as_mut() {
            meta.updated_at = now;
        }

        match proposal.action {
            MembershipActionKind::Invite => match decision.status {
                MembershipDecisionStatus::Approved => {
                    let entry = group
                        .members
                        .entry(proposal.candidate.clone())
                        .or_insert_with(|| {
                            GroupMember::new(
                                proposal.candidate.clone(),
                                proposal.requested_role.clone(),
                                MembershipStatus::Active,
                                now,
                            )
                        });
                    entry.role_id = proposal.requested_role.clone();
                    entry.status = MembershipStatus::Active;
                    entry.last_activity = now;
                    group.membership_proposals.remove(&proposal.proposal_id);
                    sync_member_membership_sets(group, &proposal.candidate, now);
                }
                MembershipDecisionStatus::Pending => {
                    group
                        .membership_proposals
                        .insert(proposal.proposal_id.clone(), proposal.clone());
                    let entry = group
                        .members
                        .entry(proposal.candidate.clone())
                        .or_insert_with(|| {
                            GroupMember::new(
                                proposal.candidate.clone(),
                                proposal.requested_role.clone(),
                                MembershipStatus::Pending,
                                now,
                            )
                        });
                    entry.role_id = proposal.requested_role.clone();
                    entry.status = MembershipStatus::Pending;
                    entry.last_activity = now;
                    sync_member_membership_sets(group, &proposal.candidate, now);
                }
                MembershipDecisionStatus::Rejected => {
                    group.membership_proposals.remove(&proposal.proposal_id);
                    group.members.remove(&proposal.candidate);
                    sync_member_membership_sets(group, &proposal.candidate, now);
                }
            },
            MembershipActionKind::Remove => match decision.status {
                MembershipDecisionStatus::Approved => {
                    if let Some(member) = group.members.get_mut(&proposal.candidate) {
                        member.status = MembershipStatus::Removed;
                        member.last_activity = now;
                    }
                    group.membership_proposals.remove(&proposal.proposal_id);
                    sync_member_membership_sets(group, &proposal.candidate, now);
                }
                MembershipDecisionStatus::Pending => {
                    group
                        .membership_proposals
                        .insert(proposal.proposal_id.clone(), proposal.clone());
                }
                MembershipDecisionStatus::Rejected => {
                    group.membership_proposals.remove(&proposal.proposal_id);
                }
            },
        }

        Ok(())
    }

    fn process_membership_proposal(
        &mut self,
        group_id: &GroupId,
        proposal: MembershipProposal,
    ) -> Result<MembershipDecision, MembershipActionError> {
        let decision = self.evaluate_membership(group_id, &proposal)?;
        let now = current_timestamp();
        self.apply_membership_decision(group_id, proposal, &decision, now)?;
        self.commit_group_crdt_or_log(group_id, "membership_proposal");
        Ok(decision)
    }
}
