// HYPERWARE CHAT APPLICATION
// A mobile-first chat application for the Hyperware platform
// Supporting 1:1 DMs, Group chats (TODO), and Voice calls (TODO)

use hyperprocess_macro::*;
use hyperware_process_lib::{
    our,
    println,
    homepage::add_to_homepage,
    http::server::{send_ws_push, WsMessageType},
    vfs,
    LazyLoadBlob,
    Address,
    ProcessId,
    Request,
    hyperapp::{SaveOptions, send, sleep, spawn},
};
use serde::{Deserialize, Serialize, Deserializer, Serializer};
use serde_json;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use flate2::write::GzEncoder;
use flate2::read::GzDecoder;
use flate2::Compression;
use std::io::{Write, Read};

// Import generated RPC functions from caller-utils
use chat_caller_utils::chat::{
    receive_chat_creation_remote_rpc,
    receive_message_remote_rpc,
    receive_message_ack_remote_rpc,
    receive_message_deletion_remote_rpc,
    receive_reaction_remote_rpc,
};
use chat_caller_utils::ChatMessage as CUChatMessage;


// Notification structures matching the notifications server API
#[derive(Serialize, Deserialize, Debug)]
enum NotificationsAction {
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

#[derive(Serialize, Deserialize, Clone, Debug)]
struct PushSubscription {
    endpoint: String,
    keys: SubscriptionKeys,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct SubscriptionKeys {
    p256dh: String,
    auth: String,
}

#[derive(Serialize, Deserialize, Debug)]
enum NotificationsResponse {
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
    pub url: String, // VFS path or data URL
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
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ChatKey {
    pub key: String,
    pub user_name: String,
    pub created_at: u64,
    pub is_revoked: bool,
    pub chat_id: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
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
            max_file_size_mb: 10, // Default 10MB limit
        }
    }
}

// WEBSOCKET MESSAGE TYPES

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum WsClientMessage {
    // Node-to-node messages
    SendMessage {
        chat_id: String,
        content: String,
        reply_to: Option<String>
    },
    Ack {
        message_id: String
    },
    MarkRead {
        chat_id: String
    },
    UpdateStatus {
        status: String
    },

    // Browser chat messages
    AuthWithKey {
        chat_key: String
    },
    BrowserMessage {
        content: String
    },

    // Common
    Heartbeat,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum WsServerMessage {
    // Node-to-node messages
    NewMessage(ChatMessage),
    MessageAck {
        message_id: String
    },
    StatusUpdate {
        node: String,
        status: String
    },
    ChatUpdate(Chat),
    ProfileUpdate {
        node: String,
        profile: UserProfile,
    },

    // Browser chat messages
    AuthSuccess {
        chat_id: String,
        history: Vec<ChatMessage>
    },
    AuthFailed {
        reason: String
    },

    // Common
    Heartbeat,
    Error {
        message: String
    },
}

// REQUEST TYPES FOR HTTP ENDPOINTS

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
    pub delete_for_both: Option<bool>, // true = delete for both, false/None = delete locally only
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
    pub data: String, // base64 encoded
    pub reply_to: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct UploadProfilePictureReq {
    pub mime_type: String,
    pub data: String, // base64 encoded
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SendVoiceNoteReq {
    pub chat_id: String,
    pub audio_data: String, // base64 encoded
    pub duration: u32, // in seconds
    pub reply_to: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SearchChatsReq {
    pub query: String,
}

// just the ones we care about
#[derive(Serialize, Deserialize, Clone, Debug, process_macros::SerdeJsonInto)]
enum HomepageRequest {
    GetPushSubscription
}

// just the ones we care about
#[derive(Serialize, Deserialize, Clone, Debug, process_macros::SerdeJsonInto)]
enum HomepageResponse {
    PushSubscription(Option<String>)
}

// APP STATE

#[derive(Serialize, Deserialize)]
pub struct ChatState {
    pub profile: UserProfile,
    pub chats: HashMap<String, Chat>,
    pub chat_keys: HashMap<String, ChatKey>,
    pub settings: Settings,
    #[serde(with = "arc_mutex_serde")]
    pub delivery_queue: Arc<Mutex<HashMap<String, Vec<ChatMessage>>>>,
    pub online_nodes: HashSet<String>,
    pub ws_connections: HashMap<u32, String>, // channel_id -> node/browser_id
    pub browser_connections: HashMap<String, u32>, // chat_key -> channel_id
    pub last_heartbeat: HashMap<u32, u64>, // channel_id -> timestamp
    #[serde(default)]
    pub active_connections: HashSet<u32>, // channel_ids that are actively viewing the app
}

fn default_delivery_queue() -> Arc<Mutex<HashMap<String, Vec<ChatMessage>>>> {
    Arc::new(Mutex::new(HashMap::new()))
}

impl Default for ChatState {
    fn default() -> Self {
        ChatState {
            profile: UserProfile::default(),
            chats: HashMap::new(),
            chat_keys: HashMap::new(),
            settings: Settings::default(),
            delivery_queue: default_delivery_queue(),
            online_nodes: HashSet::new(),
            ws_connections: HashMap::new(),
            browser_connections: HashMap::new(),
            last_heartbeat: HashMap::new(),
            active_connections: HashSet::new(),
        }
    }
}

impl Default for UserProfile {
    fn default() -> Self {
        UserProfile {
            name: "User".to_string(),
            profile_pic: None,
        }
    }
}

const OUR_PROCESS_ID: (&str, &str, &str) = ("chat", "chat", "ware.hypr");
const ICON: &str = include_str!("./icon");

// Helper function to enforce one-way status transitions
fn safe_update_message_status(current: &MessageStatus, new: MessageStatus) -> MessageStatus {
    use MessageStatus::*;

    // Define valid transitions
    match (current, &new) {
        // From Sending, can go to Sent, Delivered, or Failed
        (Sending, Sent) | (Sending, Delivered) | (Sending, Failed) => new,

        // From Sent, can only go to Delivered or Failed
        (Sent, Delivered) | (Sent, Failed) => new,

        // From Delivered, cannot change (terminal state)
        (Delivered, _) => {
            println!("WARNING: Attempted invalid status transition from Delivered to {:?}", new);
            current.clone()
        }

        // From Failed, cannot change (terminal state)
        (Failed, _) => {
            println!("WARNING: Attempted invalid status transition from Failed to {:?}", new);
            current.clone()
        }

        // Any backwards transition is invalid
        _ => {
            println!("WARNING: Attempted invalid status transition from {:?} to {:?}", current, new);
            current.clone()
        }
    }
}

// Helper functions for compression
fn compress_data(data: &[u8]) -> Result<Vec<u8>, String> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(data).map_err(|e| format!("Compression error: {}", e))?;
    encoder.finish().map_err(|e| format!("Compression finish error: {}", e))
}

fn decompress_data(compressed: &[u8]) -> Result<Vec<u8>, String> {
    let mut decoder = GzDecoder::new(compressed);
    let mut decompressed = Vec::new();
    decoder.read_to_end(&mut decompressed).map_err(|e| format!("Decompression error: {}", e))?;
    Ok(decompressed)
}

// Helper functions for base64 encoding/decoding (wrapper around base64 0.21)
fn base64_encode(data: &[u8]) -> String {
    ::base64::encode(data)
}

fn base64_decode(input: &str) -> Result<Vec<u8>, ::base64::DecodeError> {
    ::base64::decode(input)
}

// Helper function to send push notification for a message
async fn send_push_notification_for_message(
    sender: &str,
    content: &str,
    chat_id: &str
) {
    // Send notification to notifications server (it will send to all registered devices)
    let notifications_address = Address::new(
        &our().node,
        ProcessId::new(Some("notifications"), "distro", "sys")
    );

    // Truncate message for notification
    let truncated_content = if content.len() > 100 {
        format!("{}...", &content[..97])
    } else {
        content.to_string()
    };

    let notification_action = NotificationsAction::SendNotification {
        title: format!("Message from {}", sender),
        body: truncated_content,
        icon: Some("/icon-180.png".to_string()),
        data: Some(serde_json::json!({
            "url": format!("/chat#{}", chat_id),
            "chat_id": chat_id,
            "sender": sender,
            "appId": "chat:chat:ware.hypr",
            "appLabel": "Chat"
        })),
    };

    // Send the notification request
    println!("Sending notification to notifications:distro:sys");
    let request = Request::to(notifications_address.clone())
        .body(serde_json::to_vec(&notification_action).unwrap())
        .expects_response(5);

    match send::<NotificationsResponse>(request).await {
        Ok(resp) => {
            println!("Push notification response: {:?}", resp);
            match resp {
                NotificationsResponse::NotificationSent => {
                    println!("Push notification sent successfully");
                }
                NotificationsResponse::Err(e) => {
                    println!("Notification server error: {}", e);
                    // Check if the error contains "EndpointNotValid"
                    if e.contains("EndpointNotValid") {
                        // Extract the endpoint URL from the error message
                        // Error format: "Failed to send to https://fcm.googleapis.com/fcm/send/...: EndpointNotValid"
                        if let Some(start) = e.find("https://") {
                            if let Some(end) = e[start..].find(':') {
                                let endpoint = &e[start..start + end];
                                println!("Removing invalid endpoint: {}", endpoint);
                                
                                // Send request to remove the invalid subscription
                                let remove_action = NotificationsAction::RemoveSubscription {
                                    endpoint: endpoint.to_string(),
                                };
                                
                                let remove_request = Request::to(notifications_address)
                                    .body(serde_json::to_vec(&remove_action).unwrap())
                                    .expects_response(5);
                                
                                // Fire and forget the removal request
                                spawn(async move {
                                    match send::<NotificationsResponse>(remove_request).await {
                                        Ok(NotificationsResponse::SubscriptionRemoved) => {
                                            println!("Successfully removed invalid endpoint");
                                        }
                                        Ok(resp) => {
                                            println!("Unexpected response removing endpoint: {:?}", resp);
                                        }
                                        Err(e) => {
                                            println!("Error removing invalid endpoint: {:?}", e);
                                        }
                                    }
                                });
                            }
                        }
                    }
                }
                _ => {
                    println!("Unexpected notification response");
                }
            }
        }
        Err(e) => {
            println!("Error sending notification request: {:?}", e);
        }
    }
}

// HYPERPROCESS IMPLEMENTATION

#[hyperprocess(
    name = "Chat",
    ui = Some(HttpBindingConfig::default()),
    endpoints = vec![
        Binding::Http {
            path: "/api",
            config: HttpBindingConfig::default(),
        },
        Binding::Ws {
            path: "/ws",
            config: WsBindingConfig::default(),
        },
        Binding::Http {
            path: "/public",
            config: HttpBindingConfig::new(false, false, false, None)
        },
        Binding::Http {
            path: "/files/*",
            config: HttpBindingConfig::default(),
        }
    ],
    save_config = SaveOptions::OnDiff,
    wit_world = "chat-ware-dot-hypr-v0"
)]
impl ChatState {
    #[init]
    async fn initialize(&mut self) {
        add_to_homepage("Chat", Some(ICON), Some("/"), None);

        // Initialize with default profile
        if self.profile.name == "User" {
            let our_node = our().node.clone();
            self.profile.name = our_node.split('.').next().unwrap_or("User").to_string();
        }

        // Create VFS drive for storing chat files
        let package_id = our().package_id();
        match vfs::create_drive(package_id, "files", Some(5)) {
            Ok(drive_path) => {
                println!("Created files drive at: {}", drive_path);
            }
            Err(e) => {
                println!("Failed to create files drive (may already exist): {:?}", e);
            }
        }

        // Add a welcome chat if no chats exist
        if self.chats.is_empty() {
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();

            let welcome_chat = Chat {
                id: "system:welcome".to_string(),
                counterparty: "System".to_string(),
                messages: vec![ChatMessage {
                    id: format!("welcome_{}", timestamp),
                    sender: "System".to_string(),
                    content: "Welcome to Hyperware Chat! You can create new chats by clicking the + button.".to_string(),
                    timestamp,
                    status: MessageStatus::Delivered,
                    reply_to: None,
                    reactions: Vec::new(),
                    message_type: MessageType::Text,
                    file_info: None,
                }],
                last_activity: timestamp,
                unread_count: 0,
                is_blocked: false,
                notify: false,
            };

            self.chats.insert("system:welcome".to_string(), welcome_chat);
        }

        // Clone the delivery queue Arc for the spawn task
        let delivery_queue = self.delivery_queue.clone();

        // Spawn a task to periodically process the delivery queue
        spawn(async move {
            loop {
                // Wait 30 seconds between delivery attempts
                let _ = sleep(30000).await;

                // Process the delivery queue
                let queue_snapshot = {
                    let queue = delivery_queue.lock().unwrap();
                    queue.clone()
                };

                for (node, messages) in queue_snapshot {
                    if let Some(msg) = messages.first() {
                        let target = Address::from((node.as_str(), OUR_PROCESS_ID));

                        // Try to send using generated RPC method
                        let msg_json = serde_json::to_value(&msg).unwrap();
                        let msg_for_rpc: CUChatMessage = serde_json::from_value(msg_json).unwrap();

                        match receive_message_remote_rpc(&target, msg_for_rpc.clone()).await {
                            Ok(_) => {
                                println!("Successfully delivered queued message {} to {}", msg.id, node);
                                // Remove from queue if successful
                                let mut queue = delivery_queue.lock().unwrap();
                                if let Some(node_queue) = queue.get_mut(&node) {
                                    node_queue.retain(|m| m.id != msg.id);
                                    if node_queue.is_empty() {
                                        queue.remove(&node);
                                    }
                                }
                                // Note: Status update will happen when the ACK is received
                            }
                            Err(e) => {
                                // Don't attempt more messages to this node if we get Offline or Timeout
                                println!("Failed to deliver queued message to {}: {:?}", node, e);
                            }
                        }
                    }
                }
            }
        });

        println!("Chat app initialized on node: {} with {} chats", our().node, self.chats.len());
    }

    // CHAT MANAGEMENT ENDPOINTS

    #[local]
    #[http]
    async fn create_chat(&mut self, req: CreateChatReq) -> Result<Chat, String> {

        // Normalize chat ID to always be alphabetically sorted
        let chat_id = Self::normalize_chat_id(&our().node, &req.counterparty);
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let chat = Chat {
            id: chat_id.clone(),
            counterparty: req.counterparty.clone(),
            messages: Vec::new(),
            last_activity: timestamp,
            unread_count: 0,
            is_blocked: false,
            notify: true,
        };

        self.chats.insert(chat_id, chat.clone());

        // Notify the counterparty about the chat creation asynchronously
        let target = Address::from((req.counterparty.as_str(), OUR_PROCESS_ID));
        let our_node = our().node.clone();

        // Spawn task to notify counterparty without blocking
        spawn(async move {
            match receive_chat_creation_remote_rpc(&target, our_node).await {
                Ok(_) => println!("Successfully notified counterparty about chat creation"),
                Err(e) => println!("Failed to notify counterparty about chat creation: {:?}", e),
            }
        });

        Ok(chat)
    }

    #[http]
    async fn get_chats(&self) -> Result<Vec<Chat>, String> {
        let mut chats: Vec<Chat> = self.chats.values().cloned().collect();
        println!("get_chats: Returning {} chats", chats.len());
        for chat in &chats {
            println!("  Chat: {} with {}", chat.id, chat.counterparty);
        }
        chats.sort_by(|a, b| b.last_activity.cmp(&a.last_activity));

        Ok(chats)
    }

    #[http]
    async fn get_chat(&self, req: GetChatReq) -> Result<Chat, String> {

        self.chats.get(&req.chat_id)
            .cloned()
            .ok_or_else(|| "Chat not found".to_string())
    }

    #[http]
    async fn get_messages(&self, req: GetMessagesReq) -> Result<Vec<ChatMessage>, String> {
        // Get the chat
        let chat = self.chats.get(&req.chat_id)
            .ok_or_else(|| "Chat not found".to_string())?;

        // Filter messages based on timestamp if provided
        let mut messages: Vec<ChatMessage> = if let Some(before_ts) = req.before_timestamp {
            chat.messages.iter()
                .filter(|msg| msg.timestamp < before_ts)
                .cloned()
                .collect()
        } else {
            chat.messages.clone()
        };

        // Sort by timestamp descending (newest first)
        messages.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

        // Apply limit (convert u64 to usize for truncate)
        let limit = req.limit.unwrap_or(50) as usize;
        messages.truncate(limit);

        // Return in ascending order (oldest first) for display
        messages.reverse();

        Ok(messages)
    }

    #[http]
    async fn delete_chat(&mut self, req: DeleteChatReq) -> Result<String, String> {

        self.chats.remove(&req.chat_id)
            .ok_or_else(|| "Chat not found".to_string())
            .map(|_| "Chat deleted".to_string())
    }

    // MESSAGE OPERATIONS

    #[local]
    #[http]
    async fn send_message(&mut self, req: SendMessageReq) -> Result<ChatMessage, String> {

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let message_id = format!("{}:{}", timestamp, rand::random::<u32>());

        let message = ChatMessage {
            id: message_id,
            sender: our().node.clone(),
            content: req.content,
            timestamp,
            status: MessageStatus::Sending,
            reply_to: req.reply_to,
            reactions: Vec::new(),
            message_type: MessageType::Text,
            file_info: None,
        };

        // Add to chat if it exists, or create new chat
        let chat = self.chats.entry(req.chat_id.clone()).or_insert_with(|| {
            let counterparty = req.chat_id.split(':').nth(1).unwrap_or("unknown").to_string();
            Chat {
                id: req.chat_id.clone(),
                counterparty,
                messages: Vec::new(),
                last_activity: timestamp,
                unread_count: 0,
                is_blocked: false,
                notify: true,
            }
        });

        chat.messages.push(message.clone());
        chat.last_activity = timestamp;

        // Immediately update status to Sent (backend has received the message)
        if let Some(msg) = chat.messages.iter_mut().find(|m| m.id == message.id) {
            msg.status = safe_update_message_status(&msg.status, MessageStatus::Sent);
        }

        // Send ChatUpdate immediately to show Sent status
        for &channel_id in self.ws_connections.keys() {
            let chat_update = WsServerMessage::ChatUpdate(chat.clone());
            send_ws_push(channel_id, WsMessageType::Text, LazyLoadBlob {
                mime: Some("application/json".to_string()),
                bytes: serde_json::to_string(&chat_update).unwrap().into_bytes(),
            });
        }

        // Send to counterparty via P2P using generated RPC
        let counterparty = chat.counterparty.clone();
        let msg_to_send = message.clone();
        let message_id_clone = message.id.clone();
        let delivery_queue = self.delivery_queue.clone();

        let target = Address::from((counterparty.as_str(), OUR_PROCESS_ID));

        // Spawn task to attempt delivery without blocking
        spawn(async move {
            // Try to send using generated RPC method and queue if it fails
            let msg_json = serde_json::to_value(&msg_to_send).unwrap();
            let msg_for_rpc: CUChatMessage = serde_json::from_value(msg_json).unwrap();
            match receive_message_remote_rpc(&target, msg_for_rpc).await {
                Ok(_) => {
                    println!("Message {} sent successfully to {}", message_id_clone, counterparty);
                    // Message delivered successfully, counterparty will send ACK
                }
                Err(_) => {
                    println!("Failed to send message {} to {}, adding to delivery queue", message_id_clone, counterparty);
                    // Failed to send immediately, add to delivery queue
                    let mut queue = delivery_queue.lock().unwrap();
                    queue.entry(counterparty.clone())
                        .or_insert_with(Vec::new)
                        .push(msg_to_send);
                }
            }
        });

        // Return the message with updated status
        if let Some(chat) = self.chats.get(&req.chat_id) {
            if let Some(updated_msg) = chat.messages.iter().find(|m| m.id == message.id) {
                return Ok(updated_msg.clone());
            }
        }

        Ok(message)
    }

    #[http]
    async fn edit_message(&mut self, req: EditMessageReq) -> Result<String, String> {

        // Find message in the specified chat
        if let Some(chat) = self.chats.get_mut(&req.chat_id) {
            if let Some(message) = chat.messages.iter_mut().find(|m| m.id == req.message_id) {
                message.content = req.new_content;
                return Ok("Message edited".to_string());
            }
        }

        Err("Message not found".to_string())
    }

    #[http]
    async fn delete_message(&mut self, req: DeleteMessageReq) -> Result<String, String> {

        // Find and remove message from the specified chat
        if let Some(chat) = self.chats.get_mut(&req.chat_id) {
            if let Some(pos) = chat.messages.iter().position(|m| m.id == req.message_id) {
                // Store counterparty before removing message
                let counterparty = chat.counterparty.clone();
                let message_id = req.message_id.clone();
                let chat_id = req.chat_id.clone();
                let delete_for_both = req.delete_for_both.unwrap_or(false);

                // Remove the message
                chat.messages.remove(pos);

                // Notify all WebSocket connections about the updated chat
                for &channel_id in self.ws_connections.keys() {
                    let chat_update = WsServerMessage::ChatUpdate(chat.clone());
                    send_ws_push(channel_id, WsMessageType::Text, LazyLoadBlob {
                        mime: Some("application/json".to_string()),
                        bytes: serde_json::to_string(&chat_update).unwrap().into_bytes(),
                    });
                }

                // Only send deletion notification to counterparty if deleting for both
                if delete_for_both {
                    let target = Address::from((counterparty.as_str(), OUR_PROCESS_ID));
                    spawn(async move {
                        let _ = receive_message_deletion_remote_rpc(&target, message_id, chat_id).await;
                    });
                }

                return Ok("Message deleted".to_string());
            }
        }

        Err("Message not found".to_string())
    }

    #[http]
    async fn add_reaction(&mut self, req: AddReactionReq) -> Result<String, String> {

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let reaction = MessageReaction {
            emoji: req.emoji.clone(),
            user: our().node.clone(),
            timestamp,
        };

        // Find and add reaction to message in the specified chat
        if let Some(chat) = self.chats.get_mut(&req.chat_id) {
            if let Some(message) = chat.messages.iter_mut().find(|m| m.id == req.message_id) {
                // Check if user already reacted with this emoji
                if !message.reactions.iter().any(|r| r.user == reaction.user && r.emoji == reaction.emoji) {
                    message.reactions.push(reaction.clone());

                    // Send reaction to counterparty
                    // If it's their message, they need to see our reaction
                    // If it's our message, they still need to see we reacted to our own message
                    let target_node = if message.sender != our().node {
                        message.sender.clone()
                    } else {
                        // It's our message, send to the counterparty of the chat
                        chat.counterparty.clone()
                    };

                    let target = Address::new(&target_node, OUR_PROCESS_ID.clone());
                    let msg_id = req.message_id.clone();
                    let emoji = req.emoji.clone();
                    let user = our().node.clone();

                    spawn(async move {
                        match receive_reaction_remote_rpc(&target, msg_id, emoji, user).await {
                            Ok(_) => println!("Successfully sent reaction to counterparty"),
                            Err(e) => println!("Failed to send reaction to counterparty: {:?}", e),
                        }
                    });

                    // Notify WebSocket connections
                    for &channel_id in self.ws_connections.keys() {
                        let msg = WsServerMessage::ChatUpdate(chat.clone());
                        send_ws_push(channel_id, WsMessageType::Text, LazyLoadBlob {
                            mime: Some("application/json".to_string()),
                            bytes: serde_json::to_string(&msg).unwrap().into_bytes(),
                        });
                    }

                    return Ok("Reaction added".to_string());
                } else {
                    return Ok("Already reacted".to_string());
                }
            }
        }

        Err("Message not found".to_string())
    }

    #[http]
    async fn forward_message(&mut self, req: ForwardMessageReq) -> Result<ChatMessage, String> {

        // Find the message to forward from the specified chat
        let message_to_forward = self.chats.get(&req.from_chat_id)
            .and_then(|chat| chat.messages.iter().find(|m| m.id == req.message_id))
            .cloned();

        let original_message = message_to_forward.ok_or_else(|| "Message not found".to_string())?;

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let forwarded_message = ChatMessage {
            id: format!("{}:{}", timestamp, rand::random::<u32>()),
            sender: our().node.clone(),
            content: format!("Forwarded: {}", original_message.content),
            timestamp,
            status: MessageStatus::Sending,
            reply_to: None,
            reactions: Vec::new(),
            message_type: original_message.message_type.clone(),
            file_info: original_message.file_info.clone(),
        };

        // Add to destination chat
        let chat = self.chats.entry(req.to_chat_id.clone()).or_insert_with(|| {
            let counterparty = req.to_chat_id.split(':').nth(1).unwrap_or("unknown").to_string();
            Chat {
                id: req.to_chat_id.clone(),
                counterparty: counterparty.clone(),
                messages: Vec::new(),
                last_activity: timestamp,
                unread_count: 0,
                is_blocked: false,
                notify: true,
            }
        });

        chat.messages.push(forwarded_message.clone());
        chat.last_activity = timestamp;

        // Send to counterparty if it's a node-to-node chat
        if !req.to_chat_id.starts_with("browser:") {
            let counterparty = chat.counterparty.clone();
            let msg_to_send = forwarded_message.clone();

            let target = Address::from((counterparty.as_str(), OUR_PROCESS_ID));

            // Send using generated RPC method
            let msg_json = serde_json::to_value(&msg_to_send).unwrap();
            let msg_for_rpc: CUChatMessage = serde_json::from_value(msg_json).unwrap();
            match receive_message_remote_rpc(&target, msg_for_rpc).await {
                Ok(_) => {
                    if let Some(chat) = self.chats.get_mut(&req.to_chat_id) {
                        if let Some(msg) = chat.messages.iter_mut().find(|m| m.id == forwarded_message.id) {
                            msg.status = safe_update_message_status(&msg.status, MessageStatus::Sent);
                        }

                        // Send ChatUpdate with the updated message status
                        for &channel_id in self.ws_connections.keys() {
                            let chat_update = WsServerMessage::ChatUpdate(chat.clone());
                            send_ws_push(channel_id, WsMessageType::Text, LazyLoadBlob {
                                mime: Some("application/json".to_string()),
                                bytes: serde_json::to_string(&chat_update).unwrap().into_bytes(),
                            });
                        }
                    }
                }
                Err(_) => {
                    {
                        let mut queue = self.delivery_queue.lock().unwrap();
                        queue.entry(counterparty.clone())
                            .or_insert_with(Vec::new)
                            .push(msg_to_send);
                    }

                    if let Some(chat) = self.chats.get_mut(&req.to_chat_id) {
                        if let Some(msg) = chat.messages.iter_mut().find(|m| m.id == forwarded_message.id) {
                            msg.status = safe_update_message_status(&msg.status, MessageStatus::Failed);
                        }
                    }
                }
            }
        }

        Ok(forwarded_message)
    }

    #[http]
    async fn remove_reaction(&mut self, req: RemoveReactionReq) -> Result<String, String> {

        let user = our().node.clone();

        // Find and remove reaction from message
        if let Some(chat) = self.chats.get_mut(&req.chat_id) {
            if let Some(message) = chat.messages.iter_mut().find(|m| m.id == req.message_id) {
                if let Some(pos) = message.reactions.iter().position(|r| r.user == user && r.emoji == req.emoji) {
                    message.reactions.remove(pos);

                    // Notify WebSocket connections
                    for &channel_id in self.ws_connections.keys() {
                        let msg = WsServerMessage::ChatUpdate(chat.clone());
                        send_ws_push(channel_id, WsMessageType::Text, LazyLoadBlob {
                            mime: Some("application/json".to_string()),
                            bytes: serde_json::to_string(&msg).unwrap().into_bytes(),
                        });
                    }

                    return Ok("Reaction removed".to_string());
                }
            }
        }

        Err("Reaction not found".to_string())
    }

    // BROWSER CHAT MANAGEMENT

    #[http]
    async fn create_chat_link(&mut self, req: CreateChatLinkReq) -> Result<String, String> {

        let key = format!("{:x}", rand::random::<u128>());
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let chat_key = ChatKey {
            key: key.clone(),
            user_name: format!("Guest-{}", rand::random::<u32>() % 10000),
            created_at: timestamp,
            is_revoked: false,
            chat_id: req.chat_id.clone(),
        };

        self.chat_keys.insert(key.clone(), chat_key);

        let link = format!("http://{}/public/join-{}", our().node, key);
        Ok(link)
    }

    #[http]
    async fn get_chat_keys(&self) -> Result<Vec<ChatKey>, String> {
        Ok(self.chat_keys.values()
            .filter(|k| !k.is_revoked)
            .cloned()
            .collect())
    }

    #[http]
    async fn revoke_chat_key(&mut self, req: RevokeChatKeyReq) -> Result<String, String> {

        self.chat_keys.get_mut(&req.key)
            .ok_or_else(|| "Chat key not found".to_string())
            .map(|key| {
                key.is_revoked = true;
                "Chat key revoked".to_string()
            })
    }

    // SETTINGS

    #[http]
    async fn get_settings(&self) -> Result<Settings, String> {
        Ok(self.settings.clone())
    }

    #[http]
    async fn update_settings(&mut self, settings: Settings) -> Result<String, String> {
        self.settings = settings;
        Ok("Settings updated".to_string())
    }

    #[http]
    async fn update_profile(&mut self, profile: UserProfile) -> Result<String, String> {
        self.profile = profile;
        Ok("Profile updated".to_string())
    }

    #[http]
    async fn upload_profile_picture(&mut self, req: UploadProfilePictureReq) -> Result<String, String> {

        // Validate mime type
        if !req.mime_type.starts_with("image/") {
            return Err("Invalid image type".to_string());
        }

        // Store the image data as a data URL
        let data_url = format!("data:{};base64,{}", req.mime_type, req.data);
        self.profile.profile_pic = Some(data_url.clone());

        // Notify all WebSocket connections about profile update
        for &channel_id in self.ws_connections.keys() {
            let msg = WsServerMessage::ProfileUpdate {
                node: our().node.clone(),
                profile: self.profile.clone(),
            };
            send_ws_push(channel_id, WsMessageType::Text, LazyLoadBlob {
                mime: Some("application/json".to_string()),
                bytes: serde_json::to_string(&msg).unwrap().into_bytes(),
            });
        }

        Ok(data_url)
    }

    #[http]
    async fn get_profile(&self) -> Result<UserProfile, String> {
        Ok(self.profile.clone())
    }

    // FILE AND VOICE NOTE OPERATIONS

    #[http]
    async fn upload_file(&mut self, req: UploadFileReq) -> Result<ChatMessage, String> {

        // Decode base64 data
        let file_data = base64_decode(&req.data)
            .map_err(|e| format!("Failed to decode base64: {}", e))?;

        // Check file size limit
        let file_size_mb = (file_data.len() as u64) / (1024 * 1024);
        if file_size_mb > self.settings.max_file_size_mb {
            return Err(format!("File size exceeds limit of {} MB", self.settings.max_file_size_mb));
        }

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let message_id = format!("{}:{}", timestamp, rand::random::<u32>());

        // Determine message type based on mime type
        let message_type = if req.mime_type.starts_with("image/") {
            MessageType::Image
        } else {
            MessageType::File
        };

        // Store file in VFS
        let package_id = our().package_id();
        let _safe_filename = req.filename.replace("/", "_").replace("..", "_");
        let file_id = format!("{}_{}", timestamp, rand::random::<u32>());
        let vfs_path = format!("/{}/files/{}/{}",
            package_id,
            req.chat_id.replace(":", "_"),
            file_id
        );

        // Create directory if it doesn't exist
        let dir_path = format!("/{}/files/{}", package_id, req.chat_id.replace(":", "_"));
        let _ = vfs::open_dir(&dir_path, true, Some(5));

        // Create and write original file to VFS
        let file = vfs::create_file(&vfs_path, Some(5))
            .map_err(|e| format!("Failed to create VFS file: {:?}", e))?;
        file.write(&file_data)
            .map_err(|e| format!("Failed to write to VFS: {:?}", e))?;

        // For images, use data URL (they're usually small enough)
        // For other files, compress and send, or provide download link
        let (file_url, compressed_data) = if message_type == MessageType::Image {
            // Images: use data URL for easy inline display
            (format!("data:{};base64,{}", req.mime_type, req.data), None)
        } else {
            // Files: compress and prepare for sending
            let compressed = compress_data(&file_data)?;
            let compressed_b64 = base64_encode(&compressed);

            // Store compressed data for sending to counterparty
            // But locally, we'll serve from VFS
            let local_url = format!("/files/{}/{}", req.chat_id.replace(":", "_"), file_id);
            (local_url, Some(compressed_b64))
        };

        let file_info = FileInfo {
            filename: req.filename.clone(),
            mime_type: req.mime_type.clone(),
            size: file_data.len() as u64,
            url: file_url.clone(),
        };

        let message = ChatMessage {
            id: message_id,
            sender: our().node.clone(),
            content: req.filename,
            timestamp,
            status: MessageStatus::Sending,
            reply_to: req.reply_to,
            reactions: Vec::new(),
            message_type: message_type.clone(),
            file_info: Some(file_info),
        };

        // Add to chat
        let chat = self.chats.entry(req.chat_id.clone()).or_insert_with(|| {
            let counterparty = req.chat_id.split(':').nth(1).unwrap_or("unknown").to_string();
            Chat {
                id: req.chat_id.clone(),
                counterparty,
                messages: Vec::new(),
                last_activity: timestamp,
                unread_count: 0,
                is_blocked: false,
                notify: true,
            }
        });

        chat.messages.push(message.clone());
        chat.last_activity = timestamp;

        // Send to counterparty using generated RPC
        let counterparty = chat.counterparty.clone();
        let mut msg_to_send = message.clone();

        // For files (not images), replace URL with compressed data for transmission
        if message_type == MessageType::File {
            if let Some(compressed) = compressed_data {
                if let Some(ref mut file_info) = msg_to_send.file_info {
                    file_info.url = format!("compressed:{}", compressed);
                }
            }
        }

        let target = Address::from((counterparty.as_str(), OUR_PROCESS_ID));

        // Send using generated RPC method
        // Convert our local type to the generated type via JSON serialization
        let msg_json = serde_json::to_value(&msg_to_send).unwrap();
        let msg_for_rpc: CUChatMessage = serde_json::from_value(msg_json).unwrap();
        match receive_message_remote_rpc(&target, msg_for_rpc).await {
            Ok(_) => {
                if let Some(chat) = self.chats.get_mut(&req.chat_id) {
                    if let Some(msg) = chat.messages.iter_mut().find(|m| m.id == message.id) {
                        msg.status = safe_update_message_status(&msg.status, MessageStatus::Sent);
                    }

                    // Send ChatUpdate with the updated message status
                    for &channel_id in self.ws_connections.keys() {
                        let chat_update = WsServerMessage::ChatUpdate(chat.clone());
                        send_ws_push(channel_id, WsMessageType::Text, LazyLoadBlob {
                            mime: Some("application/json".to_string()),
                            bytes: serde_json::to_string(&chat_update).unwrap().into_bytes(),
                        });
                    }
                }
            }
            Err(_) => {
                {
                    let mut queue = self.delivery_queue.lock().unwrap();
                    queue.entry(counterparty.clone())
                        .or_insert_with(Vec::new)
                        .push(msg_to_send);
                }

                // Still broadcast NewMessage for failed sends
                for &channel_id in self.ws_connections.keys() {
                    let msg = WsServerMessage::NewMessage(message.clone());
                    send_ws_push(channel_id, WsMessageType::Text, LazyLoadBlob {
                        mime: Some("application/json".to_string()),
                        bytes: serde_json::to_string(&msg).unwrap().into_bytes(),
                    });
                }
            }
        }

        Ok(message)
    }
    #[http]
    async fn send_voice_note(&mut self, req: SendVoiceNoteReq) -> Result<ChatMessage, String> {

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let message_id = format!("{}:{}", timestamp, rand::random::<u32>());

        // Store voice note
        let file_url = format!("data:audio/webm;base64,{}", req.audio_data);

        let file_info = FileInfo {
            filename: format!("voice_note_{}.webm", message_id),
            mime_type: "audio/webm".to_string(),
            size: req.audio_data.len() as u64,
            url: file_url,
        };

        let message = ChatMessage {
            id: message_id,
            sender: our().node.clone(),
            content: format!("Voice note ({}s)", req.duration),
            timestamp,
            status: MessageStatus::Sending,
            reply_to: req.reply_to,
            reactions: Vec::new(),
            message_type: MessageType::VoiceNote,
            file_info: Some(file_info),
        };

        // Add to chat
        let chat = self.chats.entry(req.chat_id.clone()).or_insert_with(|| {
            let counterparty = req.chat_id.split(':').nth(1).unwrap_or("unknown").to_string();
            Chat {
                id: req.chat_id.clone(),
                counterparty,
                messages: Vec::new(),
                last_activity: timestamp,
                unread_count: 0,
                is_blocked: false,
                notify: true,
            }
        });

        chat.messages.push(message.clone());
        chat.last_activity = timestamp;

        // Send to counterparty using generated RPC
        let counterparty = chat.counterparty.clone();
        let msg_to_send = message.clone();

        let target = Address::from((counterparty.as_str(), OUR_PROCESS_ID));

        // Send using generated RPC method
        // Convert our local type to the generated type via JSON serialization
        let msg_json = serde_json::to_value(&msg_to_send).unwrap();
        let msg_for_rpc: CUChatMessage = serde_json::from_value(msg_json).unwrap();
        match receive_message_remote_rpc(&target, msg_for_rpc).await {
            Ok(_) => {
                if let Some(chat) = self.chats.get_mut(&req.chat_id) {
                    if let Some(msg) = chat.messages.iter_mut().find(|m| m.id == message.id) {
                        msg.status = safe_update_message_status(&msg.status, MessageStatus::Sent);
                    }

                    // Send ChatUpdate with the updated message status
                    for &channel_id in self.ws_connections.keys() {
                        let chat_update = WsServerMessage::ChatUpdate(chat.clone());
                        send_ws_push(channel_id, WsMessageType::Text, LazyLoadBlob {
                            mime: Some("application/json".to_string()),
                            bytes: serde_json::to_string(&chat_update).unwrap().into_bytes(),
                        });
                    }
                }
            }
            Err(_) => {
                {
                    let mut queue = self.delivery_queue.lock().unwrap();
                    queue.entry(counterparty.clone())
                        .or_insert_with(Vec::new)
                        .push(msg_to_send);
                }

                // Still broadcast NewMessage for failed sends
                for &channel_id in self.ws_connections.keys() {
                    let msg = WsServerMessage::NewMessage(message.clone());
                    send_ws_push(channel_id, WsMessageType::Text, LazyLoadBlob {
                        mime: Some("application/json".to_string()),
                        bytes: serde_json::to_string(&msg).unwrap().into_bytes(),
                    });
                }
            }
        }

        Ok(message)
    }

    // P2P MESSAGE RECEIVING

    #[remote]
    async fn receive_chat_creation(&mut self, counterparty: String) -> Result<(), String> {
        println!("receive_chat_creation: Got request from {}", counterparty);

        // Normalize chat ID to always be alphabetically sorted
        let chat_id = Self::normalize_chat_id(&counterparty, &our().node);
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Check if chat already exists
        let chat_exists = self.chats.contains_key(&chat_id);
        if !chat_exists {
            let chat = Chat {
                id: chat_id.clone(),
                counterparty: counterparty.clone(),
                messages: Vec::new(),
                last_activity: timestamp,
                unread_count: 0,
                is_blocked: false,
                notify: true,
            };

            self.chats.insert(chat_id.clone(), chat.clone());
            println!("receive_chat_creation: Created chat {}", chat_id);

            // Notify WebSocket connections about the new chat
            println!("receive_chat_creation: WebSocket connections: {}", self.ws_connections.len());
            for &channel_id in self.ws_connections.keys() {
                println!("receive_chat_creation: Sending ChatUpdate to channel {}", channel_id);
                let chat_update = WsServerMessage::ChatUpdate(chat.clone());
                send_ws_push(channel_id, WsMessageType::Text, LazyLoadBlob {
                    mime: Some("application/json".to_string()),
                    bytes: serde_json::to_string(&chat_update).unwrap().into_bytes(),
                });
            }
        } else {
            println!("receive_chat_creation: Chat {} already exists", chat_id);
        }

        // Check if we have queued messages for this counterparty
        let queued_messages = {
            let mut queue = self.delivery_queue.lock().unwrap();
            queue.remove(&counterparty).unwrap_or_default()
        };

        if !queued_messages.is_empty() {
            println!("receive_chat_creation: Found {} queued messages for {}", queued_messages.len(), counterparty);

            // Try to deliver queued messages now that we know the counterparty is online
            let target = Address::from((counterparty.as_str(), OUR_PROCESS_ID));
            let delivery_queue = self.delivery_queue.clone();

            spawn(async move {
                for msg in queued_messages {
                    let msg_json = serde_json::to_value(&msg).unwrap();
                    let msg_for_rpc: CUChatMessage = serde_json::from_value(msg_json).unwrap();

                    match receive_message_remote_rpc(&target, msg_for_rpc).await {
                        Ok(_) => {
                            println!("Successfully delivered queued message {} to {}", msg.id, counterparty);
                        }
                        Err(e) => {
                            println!("Failed to deliver queued message {} to {}: {:?}", msg.id, counterparty, e);
                            // Re-add to queue if delivery fails
                            let mut queue = delivery_queue.lock().unwrap();
                            queue.entry(counterparty.clone())
                                .or_insert_with(Vec::new)
                                .push(msg);
                            break; // Stop trying to send more messages if one fails
                        }
                    }
                }
            });
        }

        Ok(())
    }

    #[remote]
    async fn receive_message(&mut self, message: ChatMessage) -> Result<(), String> {
        // Find or create chat for this message - normalize the ID
        let chat_id = Self::normalize_chat_id(&message.sender, &our().node);
        let is_new_chat = !self.chats.contains_key(&chat_id);

        let chat = self.chats.entry(chat_id.clone()).or_insert_with(|| {
            Chat {
                id: chat_id.clone(),
                counterparty: message.sender.clone(),
                messages: Vec::new(),
                last_activity: message.timestamp,
                unread_count: 0,
                is_blocked: false,
                notify: true,
            }
        });

        // Update message status to Delivered
        let mut updated_message = message.clone();
        updated_message.status = safe_update_message_status(&message.status, MessageStatus::Delivered);

        // If message has a file, save it to our VFS
        if let Some(ref mut file_info) = updated_message.file_info {
            let is_image = updated_message.message_type == MessageType::Image;
            let original_url = file_info.url.clone();

            let file_data = if file_info.url.starts_with("compressed:") {
                // Handle compressed file data
                let compressed_b64 = &file_info.url[11..]; // Skip "compressed:" prefix

                // Decode base64
                let compressed_data = match base64_decode(compressed_b64) {
                    Ok(data) => data,
                    Err(e) => {
                        println!("Failed to decode compressed file: {}", e);
                        vec![]
                    }
                };

                // Decompress
                match decompress_data(&compressed_data) {
                    Ok(data) => data,
                    Err(e) => {
                        println!("Failed to decompress file: {}", e);
                        vec![]
                    }
                }
            } else if file_info.url.starts_with("data:") {
                // Handle data URL (for images)
                if let Some(comma_pos) = file_info.url.find(',') {
                    let base64_data = &file_info.url[comma_pos + 1..];

                    // Decode base64
                    match base64_decode(base64_data) {
                        Ok(data) => data,
                        Err(e) => {
                            println!("Failed to decode file data: {}", e);
                            vec![]
                        }
                    }
                } else {
                    vec![]
                }
            } else {
                vec![]
            };

            if !file_data.is_empty() {
                // Save to VFS
                let package_id = our().package_id();
                let file_id = format!("{}_{}", updated_message.timestamp, rand::random::<u32>());
                let vfs_path = format!("/{}/files/{}/{}",
                    package_id,
                    chat_id.replace(":", "_"),
                    file_id
                );

                // Create directory if it doesn't exist
                let dir_path = format!("/{}/files/{}", package_id, chat_id.replace(":", "_"));
                let _ = vfs::open_dir(&dir_path, true, Some(5));

                // Create and write file
                if let Ok(file) = vfs::create_file(&vfs_path, Some(5)) {
                    let _ = file.write(&file_data);
                    println!("Saved received file {} to VFS at {}", file_info.filename, vfs_path);

                    // For images, keep the data URL for inline display
                    // For files, update to local VFS path
                    if is_image {
                        // Keep the original data URL for images
                        file_info.url = original_url;
                    } else {
                        // Update the file URL to point to our local VFS path
                        file_info.url = format!("/files/{}/{}", chat_id.replace(":", "_"), file_id);
                    }
                }
            }
        }

        // Add message to chat
        chat.messages.push(updated_message.clone());
        chat.last_activity = updated_message.timestamp;
        chat.unread_count += 1;

        // Send to WebSocket connections if any
        for &channel_id in self.ws_connections.keys() {
            // If this is a new chat, send ChatUpdate first
            if is_new_chat {
                let chat_update = WsServerMessage::ChatUpdate(chat.clone());
                send_ws_push(channel_id, WsMessageType::Text, LazyLoadBlob {
                    mime: Some("application/json".to_string()),
                    bytes: serde_json::to_string(&chat_update).unwrap().into_bytes(),
                });
            }

            // Then send the new message
            let msg = WsServerMessage::NewMessage(updated_message.clone());
            send_ws_push(channel_id, WsMessageType::Text, LazyLoadBlob {
                mime: Some("application/json".to_string()),
                bytes: serde_json::to_string(&msg).unwrap().into_bytes(),
            });
        }

        // Send push notification if user has notifications enabled AND no active connections
        // We only send notifications if the user is not actively viewing the app
        if chat.notify && self.settings.notify_chats && self.active_connections.is_empty() {
            // Try to send a push notification
            spawn(async move {
                send_push_notification_for_message(
                    &updated_message.sender,
                    &updated_message.content,
                    &chat_id
                ).await;
            });
        }

        // Send acknowledgment back to sender using generated RPC
        let sender = message.sender.clone();
        let msg_id = message.id.clone();

        let target = Address::from((sender.as_str(), OUR_PROCESS_ID));

        // Send acknowledgment using generated RPC method
        let _ = receive_message_ack_remote_rpc(&target, msg_id).await;

        Ok(())
    }

    // Remote handler for receiving reactions
    #[remote]
    async fn receive_reaction(&mut self, message_id: String, emoji: String, user: String) -> Result<(), String> {
        println!("Received reaction {} from {} for message {}", emoji, user, message_id);

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let reaction = MessageReaction {
            emoji: emoji.clone(),
            user: user.clone(),
            timestamp,
        };

        // Find the message and add the reaction
        for chat in self.chats.values_mut() {
            if let Some(message) = chat.messages.iter_mut().find(|m| m.id == message_id) {
                // Check if user already reacted with this emoji
                if !message.reactions.iter().any(|r| r.user == reaction.user && r.emoji == reaction.emoji) {
                    message.reactions.push(reaction);

                    // Send ChatUpdate to WebSocket connections
                    for &channel_id in self.ws_connections.keys() {
                        let chat_update = WsServerMessage::ChatUpdate(chat.clone());
                        send_ws_push(channel_id, WsMessageType::Text, LazyLoadBlob {
                            mime: Some("application/json".to_string()),
                            bytes: serde_json::to_string(&chat_update).unwrap().into_bytes(),
                        });
                    }
                    return Ok(());
                }
            }
        }

        // Not an error - might be a reaction for a message we don't have
        Ok(())
    }

    // Remote handler for receiving message acknowledgments
    #[remote]
    async fn receive_message_ack(&mut self, message_id: String) -> Result<(), String> {
        println!("Received ACK for message {}", message_id);
        // This ACK is from the remote node confirming they received our message
        // We need to find OUR sent message and update its status to Delivered

        // Look through all chats to find the message we sent
        for chat in self.chats.values_mut() {
            // Only look for messages where WE are the sender
            if let Some(message) = chat.messages.iter_mut()
                .find(|m| m.id == message_id && m.sender == our().node) {

                println!("Updating sent message {} status to Delivered", message_id);
                message.status = safe_update_message_status(&message.status, MessageStatus::Delivered);

                // Send ChatUpdate with the delivered status
                for &channel_id in self.ws_connections.keys() {
                    println!("Sending ChatUpdate for delivered message to channel {}", channel_id);
                    let chat_update = WsServerMessage::ChatUpdate(chat.clone());
                    send_ws_push(channel_id, WsMessageType::Text, LazyLoadBlob {
                        mime: Some("application/json".to_string()),
                        bytes: serde_json::to_string(&chat_update).unwrap().into_bytes(),
                    });
                }
                return Ok(());
            }
        }
        println!("Sent message {} not found for ACK", message_id);
        // Not an error - might be an ACK for a message we don't have anymore
        Ok(())
    }

    #[remote]
    async fn receive_message_deletion(&mut self, message_id: String, chat_id: String) -> Result<(), String> {
        println!("Received deletion request for message {} in chat {}", message_id, chat_id);

        // Find the chat and delete the message
        if let Some(chat) = self.chats.get_mut(&chat_id) {
            if let Some(pos) = chat.messages.iter().position(|m| m.id == message_id) {
                chat.messages.remove(pos);
                println!("Deleted message {} from chat {}", message_id, chat_id);

                // Notify all WebSocket connections about the updated chat
                for &channel_id in self.ws_connections.keys() {
                    let chat_update = WsServerMessage::ChatUpdate(chat.clone());
                    send_ws_push(channel_id, WsMessageType::Text, LazyLoadBlob {
                        mime: Some("application/json".to_string()),
                        bytes: serde_json::to_string(&chat_update).unwrap().into_bytes(),
                    });
                }
            }
        }

        Ok(())
    }

    // PUBLIC BROWSER CHAT ENDPOINTS

    #[http(path = "/public")]
    async fn serve_public_chat(&self) -> Result<String, String> {
        // Serve the browser chat HTML
        Ok(include_str!("../../ui/public/browser-chat.html").to_string())
    }

    #[http(path = "/public/join-*")]
    async fn serve_join_link(&self) -> Result<String, String> {
        // Serve the browser chat HTML for join links
        Ok(include_str!("../../ui/public/browser-chat.html").to_string())
    }

    #[http(path = "/files/*")]
    async fn serve_file(&self, path_segments: Vec<String>) -> Result<(String, Vec<u8>), String> {
        // Extract path from segments (should be /files/chat_id/file_id)
        if path_segments.len() < 3 {
            return Err("Invalid file path".to_string());
        }

        let chat_id = &path_segments[1];
        let file_id = &path_segments[2];

        // Build VFS path
        let package_id = our().package_id();
        let vfs_path = format!("/{}/files/{}/{}", package_id, chat_id, file_id);

        // Read file from VFS
        let file = vfs::open_file(&vfs_path, false, Some(5))
            .map_err(|e| format!("Failed to open file: {:?}", e))?;

        let file_data = file.read()
            .map_err(|e| format!("Failed to read file: {:?}", e))?;

        // Try to determine MIME type from file content or default to application/octet-stream
        let mime_type = "application/octet-stream".to_string();

        Ok((mime_type, file_data))
    }
    // SEARCH

    #[http]
    async fn search_chats(&self, req: SearchChatsReq) -> Result<Vec<Chat>, String> {

        let query = req.query.to_lowercase();
        let results: Vec<Chat> = self.chats.values()
            .filter(|chat| {
                chat.counterparty.to_lowercase().contains(&query) ||
                chat.messages.iter().any(|m| m.content.to_lowercase().contains(&query))
            })
            .cloned()
            .collect();

        Ok(results)
    }

    // WEBSOCKET HANDLERS

    #[ws]
    fn websocket(&mut self, channel_id: u32, message_type: WsMessageType, blob: LazyLoadBlob) {
        // We'll differentiate between public and private connections via authentication
        match message_type {
            WsMessageType::Close => {
                println!("WebSocket connection closed: {}", channel_id);
                // Clean up connection
                if let Some(node) = self.ws_connections.remove(&channel_id) {
                    self.online_nodes.remove(&node);
                    // Broadcast status update
                    let status_msg = WsServerMessage::StatusUpdate {
                        node: node.clone(),
                        status: "offline".to_string(),
                    };
                    self.broadcast_to_all(serde_json::to_string(&status_msg).unwrap());
                }

                // Clean up browser connections
                self.browser_connections.retain(|_, &mut v| v != channel_id);
                self.active_connections.remove(&channel_id);
            }
            WsMessageType::Text => {
                // Parse and handle client message
                if let Ok(payload) = String::from_utf8(blob.bytes.clone()) {
                    match serde_json::from_str::<WsClientMessage>(&payload) {
                        Ok(msg) => {
                            println!("WebSocket: Received message from channel {}: {:?}", channel_id, msg);
                            // Initialize connection if not already present
                            if !self.ws_connections.contains_key(&channel_id) && !self.browser_connections.values().any(|&ch| ch == channel_id) {
                                println!("WebSocket: New connection from channel {}, initializing...", channel_id);
                                self.ws_connections.insert(channel_id, our().node.clone());

                                // Send all existing chats to the new connection
                                println!("WebSocket: Sending {} chats to new connection", self.chats.len());
                                for chat in self.chats.values() {
                                    println!("WebSocket: Sending chat {} with {} messages", chat.id, chat.messages.len());
                                    let chat_update = WsServerMessage::ChatUpdate(chat.clone());
                                    send_ws_push(channel_id, WsMessageType::Text, LazyLoadBlob {
                                        mime: Some("application/json".to_string()),
                                        bytes: serde_json::to_string(&chat_update).unwrap().into_bytes(),
                                    });
                                }
                                println!("WebSocket: Initial chat sync complete for channel {}", channel_id);
                            }

                            // Check if this is a browser chat authentication
                            if let WsClientMessage::AuthWithKey { .. } = &msg {
                                self.handle_browser_message(channel_id, msg);
                            } else if self.browser_connections.values().any(|&ch| ch == channel_id) {
                                // If already authenticated as browser
                                self.handle_browser_message(channel_id, msg);
                            } else {
                                // Node-to-node message
                                self.handle_client_message(channel_id, msg);
                            }
                        }
                        Err(e) => {
                            let error = WsServerMessage::Error {
                                message: format!("Invalid message format: {}", e),
                            };
                            send_ws_push(channel_id, WsMessageType::Text, LazyLoadBlob {
                            mime: Some("application/json".to_string()),
                            bytes: serde_json::to_string(&error).unwrap().into_bytes(),
                        });
                        }
                    }
                }
            }
            WsMessageType::Binary => {
                // Handle binary messages if needed (e.g., for voice calls later)
                println!("Binary message received on channel {}", channel_id);
            }
            WsMessageType::Ping | WsMessageType::Pong => {
                // Ignore ping/pong messages
            }
        }
    }
}

// Helper methods implementation
impl ChatState {
    // Normalize chat ID to prevent duplicates
    // Always returns the ID in alphabetical order: "nodeA:nodeB"
    fn normalize_chat_id(node1: &str, node2: &str) -> String {
        if node1 < node2 {
            format!("{}:{}", node1, node2)
        } else {
            format!("{}:{}", node2, node1)
        }
    }

    async fn process_delivery_queue(&mut self) {
        let queue_len = {
            let queue = self.delivery_queue.lock().unwrap();
            queue.len()
        };
        println!("Processing delivery queue with {} nodes", queue_len);

        // Process queued messages for each node
        let nodes_to_process: Vec<String> = {
            let queue = self.delivery_queue.lock().unwrap();
            queue.keys().cloned().collect()
        };

        for node in nodes_to_process {
            // Get the first message for this node
            let msg_to_send = {
                let queue = self.delivery_queue.lock().unwrap();
                queue.get(&node).and_then(|messages| messages.first().cloned())
            };

            if let Some(msg) = msg_to_send {
                let target = Address::from((node.as_str(), OUR_PROCESS_ID));

                // Try to send using generated RPC method
                let msg_json = serde_json::to_value(&msg).unwrap();
                let msg_for_rpc: CUChatMessage = serde_json::from_value(msg_json).unwrap();

                match receive_message_remote_rpc(&target, msg_for_rpc.clone()).await {
                    Ok(_) => {
                        println!("Successfully delivered queued message {} to {}", msg.id, node);
                        // Remove from queue if successful
                        {
                            let mut queue = self.delivery_queue.lock().unwrap();
                            if let Some(node_queue) = queue.get_mut(&node) {
                                node_queue.retain(|m| m.id != msg.id);
                                if node_queue.is_empty() {
                                    queue.remove(&node);
                                }
                            }
                        }

                        // Update message status in our chat
                        for chat in self.chats.values_mut() {
                            if let Some(message) = chat.messages.iter_mut().find(|m| m.id == msg.id) {
                                message.status = safe_update_message_status(&message.status, MessageStatus::Sent);

                                // Send ChatUpdate to WebSocket connections
                                for &channel_id in self.ws_connections.keys() {
                                    let chat_update = WsServerMessage::ChatUpdate(chat.clone());
                                    send_ws_push(channel_id, WsMessageType::Text, LazyLoadBlob {
                                        mime: Some("application/json".to_string()),
                                        bytes: serde_json::to_string(&chat_update).unwrap().into_bytes(),
                                    });
                                }
                                break;
                            }
                        }
                    }
                    Err(e) => {
                        // Don't attempt more messages to this node if we get Offline or Timeout
                        println!("Failed to deliver queued message to {}: {:?}", node, e);
                    }
                }
            }
        }
    }

    fn handle_client_message(&mut self, channel_id: u32, msg: WsClientMessage) {
        match msg {
            WsClientMessage::SendMessage { chat_id, content, reply_to } => {
                // Create and send message
                let timestamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs();

                let message_id = format!("{}:{}", timestamp, rand::random::<u32>());
                let sender = self.ws_connections.get(&channel_id)
                    .cloned()
                    .unwrap_or_else(|| our().node.clone());

                let message = ChatMessage {
                    id: message_id.clone(),
                    sender,
                    content,
                    timestamp,
                    status: MessageStatus::Sending,
                    reply_to,
                    reactions: Vec::new(),
                    message_type: MessageType::Text,
                    file_info: None,
                };

                // Add to chat
                if let Some(chat) = self.chats.get_mut(&chat_id) {
                    chat.messages.push(message.clone());
                    chat.last_activity = timestamp;

                    // Send to counterparty if online
                    let counterparty = chat.counterparty.clone();
                    if self.online_nodes.contains(&counterparty) {
                        // Find counterparty's channel
                        for (&ch_id, node) in &self.ws_connections {
                            if node == &counterparty {
                                let msg = WsServerMessage::NewMessage(message.clone());
                                send_ws_push(ch_id, WsMessageType::Text, LazyLoadBlob {
                                    mime: Some("application/json".to_string()),
                                    bytes: serde_json::to_string(&msg).unwrap().into_bytes(),
                                });
                                break;
                            }
                        }
                    } else {
                        // Queue for delivery
                        {
                            let mut queue = self.delivery_queue.lock().unwrap();
                            queue.entry(counterparty)
                                .or_insert_with(Vec::new)
                                .push(message.clone());
                        }
                    }

                    // Update status to Sent now that BE has received and processed it
                    if let Some(msg) = chat.messages.iter_mut().find(|m| m.id == message_id) {
                        msg.status = safe_update_message_status(&msg.status, MessageStatus::Sent);
                    }

                    // Send ChatUpdate with the updated status
                    let chat_update = WsServerMessage::ChatUpdate(chat.clone());
                    send_ws_push(channel_id, WsMessageType::Text, LazyLoadBlob {
                        mime: Some("application/json".to_string()),
                        bytes: serde_json::to_string(&chat_update).unwrap().into_bytes(),
                    });
                }

                // Send acknowledgment
                let ack = WsServerMessage::MessageAck { message_id };
                send_ws_push(channel_id, WsMessageType::Text, LazyLoadBlob {
                    mime: Some("application/json".to_string()),
                    bytes: serde_json::to_string(&ack).unwrap().into_bytes(),
                });
            }
            WsClientMessage::Ack { message_id } => {
                // Update message status
                for chat in self.chats.values_mut() {
                    if let Some(message) = chat.messages.iter_mut().find(|m| m.id == message_id) {
                        message.status = safe_update_message_status(&message.status, MessageStatus::Delivered);
                        break;
                    }
                }
            }
            WsClientMessage::MarkRead { chat_id } => {
                if let Some(chat) = self.chats.get_mut(&chat_id) {
                    chat.unread_count = 0;
                }
            }
            WsClientMessage::UpdateStatus { status } => {
                // Track whether this connection is active (user viewing the page)
                if status == "active" {
                    self.active_connections.insert(channel_id);
                } else if status == "inactive" {
                    self.active_connections.remove(&channel_id);
                }

                if let Some(node) = self.ws_connections.get(&channel_id) {
                    let msg = WsServerMessage::StatusUpdate {
                        node: node.clone(),
                        status,
                    };
                    self.broadcast_to_all(serde_json::to_string(&msg).unwrap());
                }
            }
            WsClientMessage::Heartbeat => {
                let msg = WsServerMessage::Heartbeat;
                send_ws_push(channel_id, WsMessageType::Text, LazyLoadBlob {
                    mime: Some("application/json".to_string()),
                    bytes: serde_json::to_string(&msg).unwrap().into_bytes(),
                });
            }
            _ => {
                // Other message types not handled in node-to-node
            }
        }
    }

    fn handle_browser_message(&mut self, channel_id: u32, msg: WsClientMessage) {
        match msg {
            WsClientMessage::AuthWithKey { chat_key } => {
                if let Some(key_data) = self.chat_keys.get(&chat_key) {
                    if !key_data.is_revoked {
                        // Store connection
                        self.browser_connections.insert(chat_key.clone(), channel_id);

                        // Get chat history
                        let history = self.chats.get(&key_data.chat_id)
                            .map(|chat| chat.messages.clone())
                            .unwrap_or_default();

                        let msg = WsServerMessage::AuthSuccess {
                            chat_id: key_data.chat_id.clone(),
                            history,
                        };
                        send_ws_push(channel_id, WsMessageType::Text, LazyLoadBlob {
                    mime: Some("application/json".to_string()),
                    bytes: serde_json::to_string(&msg).unwrap().into_bytes(),
                });
                    } else {
                        let msg = WsServerMessage::AuthFailed {
                            reason: "Chat key has been revoked".to_string(),
                        };
                        send_ws_push(channel_id, WsMessageType::Text, LazyLoadBlob {
                    mime: Some("application/json".to_string()),
                    bytes: serde_json::to_string(&msg).unwrap().into_bytes(),
                });
                    }
                } else {
                    let msg = WsServerMessage::AuthFailed {
                        reason: "Invalid chat key".to_string(),
                    };
                    send_ws_push(channel_id, WsMessageType::Text, LazyLoadBlob {
                    mime: Some("application/json".to_string()),
                    bytes: serde_json::to_string(&msg).unwrap().into_bytes(),
                });
                }
            }
            WsClientMessage::BrowserMessage { content } => {
                // Find chat key for this connection
                if let Some((chat_key, _)) = self.browser_connections.iter().find(|(_, &ch)| ch == channel_id) {
                    if let Some(key_data) = self.chat_keys.get(chat_key) {
                        let timestamp = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_secs();

                        let message = ChatMessage {
                            id: format!("{}:{}", timestamp, rand::random::<u32>()),
                            sender: key_data.user_name.clone(),
                            content,
                            timestamp,
                            status: MessageStatus::Sent,
                            reply_to: None,
                            reactions: Vec::new(),
                            message_type: MessageType::Text,
                            file_info: None,
                        };

                        // Add to chat
                        let chat = self.chats.entry(key_data.chat_id.clone())
                            .or_insert_with(|| Chat {
                                id: key_data.chat_id.clone(),
                                counterparty: key_data.user_name.clone(),
                                messages: Vec::new(),
                                last_activity: timestamp,
                                unread_count: 0,
                                is_blocked: false,
                                notify: true,
                            });

                        chat.messages.push(message.clone());
                        chat.last_activity = timestamp;
                        chat.unread_count += 1;

                        // Send message to all participants
                        let msg = WsServerMessage::NewMessage(message);
                        send_ws_push(channel_id, WsMessageType::Text, LazyLoadBlob {
                    mime: Some("application/json".to_string()),
                    bytes: serde_json::to_string(&msg).unwrap().into_bytes(),
                });
                    }
                }
            }
            WsClientMessage::Heartbeat => {
                let msg = WsServerMessage::Heartbeat;
                send_ws_push(channel_id, WsMessageType::Text, LazyLoadBlob {
                    mime: Some("application/json".to_string()),
                    bytes: serde_json::to_string(&msg).unwrap().into_bytes(),
                });
            }
            _ => {}
        }
    }

    fn broadcast_to_all(&self, message: String) {
        for &channel_id in self.ws_connections.keys() {
            send_ws_push(channel_id, WsMessageType::Text, LazyLoadBlob {
                mime: Some("application/json".to_string()),
                bytes: message.clone().into_bytes(),
            });
        }
    }
}

// Add rand for generating IDs
mod rand {
    pub fn random<T>() -> T
    where
        T: From<u32>
    {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u32;
        T::from(timestamp)
    }
}

// Simple base64 decoder
mod base64 {
    pub fn decode(input: &str) -> Result<Vec<u8>, String> {
        // Remove any whitespace
        let input = input.chars().filter(|c| !c.is_whitespace()).collect::<String>();

        // Base64 character set
        const BASE64_CHARS: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

        let mut output = Vec::new();
        let mut buffer = 0u32;
        let mut bits_collected = 0;

        for c in input.chars() {
            if c == '=' {
                break; // Padding character, we're done
            }

            let value = BASE64_CHARS.find(c)
                .ok_or_else(|| format!("Invalid base64 character: {}", c))? as u32;

            buffer = (buffer << 6) | value;
            bits_collected += 6;

            while bits_collected >= 8 {
                bits_collected -= 8;
                output.push((buffer >> bits_collected) as u8);
                buffer &= (1 << bits_collected) - 1;
            }
        }

        Ok(output)
    }
}

mod arc_mutex_serde {
    use super::*;

    pub fn serialize<S, T>(val: &Arc<Mutex<T>>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
        T: Serialize,
    {
        use serde::ser::Error;
        match val.lock() {
            Ok(guard) => guard.serialize(serializer),
            Err(_) => Err(Error::custom("mutex poisoned")),
        }
    }

    pub fn deserialize<'de, D, T>(deserializer: D) -> Result<Arc<Mutex<T>>, D::Error>
    where
        D: Deserializer<'de>,
        T: Deserialize<'de>,
    {
        let data = T::deserialize(deserializer)?;
        Ok(Arc::new(Mutex::new(data)))
    }
}
