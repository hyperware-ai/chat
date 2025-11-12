use futures::channel::mpsc::{self, UnboundedReceiver, UnboundedSender};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PushSubscription {
    pub endpoint: String,
    pub keys: SubscriptionKeys,
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

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
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

#[derive(Serialize, Deserialize, Clone, Debug, process_macros::SerdeJsonInto)]
pub enum HomepageRequest {
    GetPushSubscription,
}

#[derive(Serialize, Deserialize, Clone, Debug, process_macros::SerdeJsonInto)]
pub enum HomepageResponse {
    PushSubscription(Option<String>),
}

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
    pub pending_deliveries: Arc<Mutex<HashMap<String, Vec<ChatMessage>>>>,
    pub online_nodes: HashSet<String>,
    pub ws_connections: HashMap<u32, String>,
    pub browser_connections: HashMap<String, u32>,
    pub last_heartbeat: HashMap<u32, u64>,
    #[serde(default)]
    pub active_connections: HashSet<u32>,
    #[serde(default)]
    pub node_profiles: HashMap<String, UserProfile>,
}

impl Default for ChatState {
    fn default() -> Self {
        let (delivery_tx, delivery_rx) = DeliveryTx::new();

        ChatState {
            profile: UserProfile::default(),
            chats: HashMap::new(),
            chat_keys: HashMap::new(),
            settings: Settings::default(),
            message_sequence_counters: HashMap::new(),
            delivery_tx,
            // represents "still available" versus "already consumed"
            delivery_rx: Some(delivery_rx),
            pending_deliveries: Arc::new(Mutex::new(HashMap::new())),
            online_nodes: HashSet::new(),
            ws_connections: HashMap::new(),
            browser_connections: HashMap::new(),
            last_heartbeat: HashMap::new(),
            active_connections: HashSet::new(),
            node_profiles: HashMap::new(),
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

#[derive(Clone)]
pub struct DeliveryTx {
    sender: UnboundedSender<QueuedDelivery>,
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
        }

        let data = ChatStateSerde::deserialize(deserializer)?;
        let (delivery_tx, delivery_rx) = DeliveryTx::new();

        Ok(ChatState {
            profile: data.profile,
            chats: data.chats,
            chat_keys: data.chat_keys,
            settings: data.settings,
            message_sequence_counters: data.message_sequence_counters,
            delivery_tx,
            delivery_rx: Some(delivery_rx),
            pending_deliveries: Arc::new(Mutex::new(HashMap::new())),
            online_nodes: data.online_nodes,
            ws_connections: data.ws_connections,
            browser_connections: data.browser_connections,
            last_heartbeat: data.last_heartbeat,
            active_connections: data.active_connections,
            node_profiles: data.node_profiles,
        })
    }
}
