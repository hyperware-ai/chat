// HYPERWARE CHAT APPLICATION
// A mobile-first chat application for the Hyperware platform
// Supporting 1:1 DMs, Group chats (TODO), and Voice calls (TODO)

use hyperprocess_macro::*;
use hyperware_process_lib::{
    our,
    println,
    homepage::add_to_homepage,
    http::server::{send_ws_push, WsMessageType},
    LazyLoadBlob,
    Request,
    hyperapp::{send, SaveOptions},
};
use serde::{Deserialize, Serialize};
use serde_json;
use std::collections::{HashMap, HashSet};

// CORE TYPES

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
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
#[serde(rename_all = "camelCase")]
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
#[serde(rename_all = "camelCase")]
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
#[serde(rename_all = "camelCase")]
pub struct ChatKey {
    pub key: String,
    pub user_name: String,
    pub created_at: u64,
    pub is_revoked: bool,
    pub chat_id: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UserProfile {
    pub name: String,
    pub profile_pic: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
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

// APP STATE

#[derive(Default, Serialize, Deserialize)]
pub struct AppState {
    pub profile: UserProfile,
    pub chats: HashMap<String, Chat>,
    pub chat_keys: HashMap<String, ChatKey>,
    pub settings: Settings,
    pub delivery_queue: HashMap<String, Vec<ChatMessage>>,
    pub online_nodes: HashSet<String>,
    pub ws_connections: HashMap<u32, String>, // channel_id -> node/browser_id
    pub browser_connections: HashMap<String, u32>, // chat_key -> channel_id
    pub last_heartbeat: HashMap<u32, u64>, // channel_id -> timestamp
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
            config: WsBindingConfig::default(), // authenticated: false for browser support
        },
        Binding::Http {
            path: "/public",
            config: HttpBindingConfig::new(false, false, false, None)
        }
    ],
    save_config = SaveOptions::OnDiff,
    wit_world = "chat-app-dot-os-v0"
)]
impl AppState {

    #[init]
    async fn initialize(&mut self) {
        add_to_homepage("Chat", None, Some("/"), None);

        // Initialize with default profile
        if self.profile.name == "User" {
            let our_node = our().node.clone();
            self.profile.name = our_node.split('.').next().unwrap_or("User").to_string();
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

        println!("Chat app initialized on node: {} with {} chats", our().node, self.chats.len());
    }

    // CHAT MANAGEMENT ENDPOINTS

    #[http]
    async fn create_chat(&mut self, request_body: String) -> Result<Chat, String> {
        #[derive(Deserialize)]
        struct CreateChatRequest {
            counterparty: String,
        }

        let req: CreateChatRequest = serde_json::from_str(&request_body)
            .map_err(|e| format!("Invalid request: {}", e))?;

        let chat_id = format!("{}:{}", our().node, req.counterparty);
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

        // Notify the counterparty about the chat creation
        let body = serde_json::json!({"ReceiveChatCreation": our().node});
        let body_bytes = serde_json::to_vec(&body).unwrap_or_default();

        let target = (req.counterparty.as_str(), OUR_PROCESS_ID);
        let request = Request::to(target)
            .body(body_bytes)
            .expects_response(5); // 5 second timeout

        // Send notification asynchronously (don't block on response)
        let _ = send::<Result<(), String>>(request).await;

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
    async fn get_chat(&self, request_body: String) -> Result<Chat, String> {
        #[derive(Deserialize)]
        struct GetChatRequest {
            chat_id: String,
        }

        let req: GetChatRequest = serde_json::from_str(&request_body)
            .map_err(|e| format!("Invalid request: {}", e))?;

        self.chats.get(&req.chat_id)
            .cloned()
            .ok_or_else(|| "Chat not found".to_string())
    }

    #[http]
    async fn delete_chat(&mut self, request_body: String) -> Result<String, String> {
        #[derive(Deserialize)]
        struct DeleteChatRequest {
            chat_id: String,
        }

        let req: DeleteChatRequest = serde_json::from_str(&request_body)
            .map_err(|e| format!("Invalid request: {}", e))?;

        self.chats.remove(&req.chat_id)
            .ok_or_else(|| "Chat not found".to_string())
            .map(|_| "Chat deleted".to_string())
    }

    // MESSAGE OPERATIONS

    #[http]
    async fn send_message(&mut self, request_body: String) -> Result<ChatMessage, String> {
        #[derive(Deserialize)]
        struct SendMessageRequest {
            chat_id: String,
            content: String,
            reply_to: Option<String>,
        }

        let req: SendMessageRequest = serde_json::from_str(&request_body)
            .map_err(|e| format!("Invalid request: {}", e))?;

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

        // Send to counterparty via P2P
        let counterparty = chat.counterparty.clone();
        let msg_to_send = message.clone();

        // Create the request body with the message
        let body = serde_json::json!({"ReceiveMessage": msg_to_send});
        let body_bytes = serde_json::to_vec(&body).unwrap_or_default();

        // Send to the counterparty node using tuple format for Address
        // Format: (node, process, package, publisher)
        let target = (counterparty.as_str(), OUR_PROCESS_ID);
        let request = Request::to(target)
            .body(body_bytes)
            .expects_response(5); // 5 second timeout

        // Try to send and queue if it fails
        match send::<Result<(), String>>(request).await {
            Ok(_) => {
                // Message sent successfully, update status
                if let Some(chat) = self.chats.get_mut(&req.chat_id) {
                    if let Some(msg) = chat.messages.iter_mut().find(|m| m.id == message.id) {
                        msg.status = MessageStatus::Sent;
                    }
                }
                
                // Notify WebSocket connections about status update
                for &channel_id in self.ws_connections.keys() {
                    let msg = WsServerMessage::MessageAck { message_id: message.id.clone() };
                    send_ws_push(channel_id, WsMessageType::Text, LazyLoadBlob {
                        mime: Some("application/json".to_string()),
                        bytes: serde_json::to_string(&msg).unwrap().into_bytes(),
                    });
                }
            }
            Err(_) => {
                // Failed to send, add to delivery queue
                self.delivery_queue
                    .entry(counterparty.clone())
                    .or_insert_with(Vec::new)
                    .push(msg_to_send);

                // Update status to failed
                if let Some(chat) = self.chats.get_mut(&req.chat_id) {
                    if let Some(msg) = chat.messages.iter_mut().find(|m| m.id == message.id) {
                        msg.status = MessageStatus::Failed;
                    }
                }
            }
        }

        // Broadcast the new message to all WebSocket connections
        for &channel_id in self.ws_connections.keys() {
            let msg = WsServerMessage::NewMessage(message.clone());
            send_ws_push(channel_id, WsMessageType::Text, LazyLoadBlob {
                mime: Some("application/json".to_string()),
                bytes: serde_json::to_string(&msg).unwrap().into_bytes(),
            });
        }

        Ok(message)
    }

    #[http]
    async fn edit_message(&mut self, request_body: String) -> Result<String, String> {
        #[derive(Deserialize)]
        struct EditMessageRequest {
            message_id: String,
            new_content: String,
        }

        let req: EditMessageRequest = serde_json::from_str(&request_body)
            .map_err(|e| format!("Invalid request: {}", e))?;

        // Find message in all chats
        for chat in self.chats.values_mut() {
            if let Some(message) = chat.messages.iter_mut().find(|m| m.id == req.message_id) {
                message.content = req.new_content;
                return Ok("Message edited".to_string());
            }
        }

        Err("Message not found".to_string())
    }

    #[http]
    async fn delete_message(&mut self, request_body: String) -> Result<String, String> {
        #[derive(Deserialize)]
        struct DeleteMessageRequest {
            message_id: String,
        }

        let req: DeleteMessageRequest = serde_json::from_str(&request_body)
            .map_err(|e| format!("Invalid request: {}", e))?;

        // Find and remove message from all chats
        for chat in self.chats.values_mut() {
            if let Some(pos) = chat.messages.iter().position(|m| m.id == req.message_id) {
                chat.messages.remove(pos);
                return Ok("Message deleted".to_string());
            }
        }

        Err("Message not found".to_string())
    }

    #[http]
    async fn add_reaction(&mut self, request_body: String) -> Result<String, String> {
        #[derive(Deserialize)]
        struct AddReactionRequest {
            message_id: String,
            emoji: String,
        }

        let req: AddReactionRequest = serde_json::from_str(&request_body)
            .map_err(|e| format!("Invalid request: {}", e))?;

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let reaction = MessageReaction {
            emoji: req.emoji,
            user: our().node.clone(),
            timestamp,
        };

        // Find and add reaction to message
        for chat in self.chats.values_mut() {
            if let Some(message) = chat.messages.iter_mut().find(|m| m.id == req.message_id) {
                // Check if user already reacted with this emoji
                if !message.reactions.iter().any(|r| r.user == reaction.user && r.emoji == reaction.emoji) {
                    message.reactions.push(reaction.clone());

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
    async fn forward_message(&mut self, request_body: String) -> Result<ChatMessage, String> {
        #[derive(Deserialize)]
        struct ForwardMessageRequest {
            message_id: String,
            to_chat_id: String,
        }

        let req: ForwardMessageRequest = serde_json::from_str(&request_body)
            .map_err(|e| format!("Invalid request: {}", e))?;

        // Find the message to forward
        let mut message_to_forward = None;
        for chat in self.chats.values() {
            if let Some(msg) = chat.messages.iter().find(|m| m.id == req.message_id) {
                message_to_forward = Some(msg.clone());
                break;
            }
        }

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

            let body = serde_json::json!({"ReceiveMessage": msg_to_send});
            let body_bytes = serde_json::to_vec(&body).unwrap_or_default();

            let target = (counterparty.as_str(), OUR_PROCESS_ID);
            let request = Request::to(target)
                .body(body_bytes)
                .expects_response(5);

            match send::<Result<(), String>>(request).await {
                Ok(_) => {
                    if let Some(chat) = self.chats.get_mut(&req.to_chat_id) {
                        if let Some(msg) = chat.messages.iter_mut().find(|m| m.id == forwarded_message.id) {
                            msg.status = MessageStatus::Sent;
                        }
                    }
                }
                Err(_) => {
                    self.delivery_queue
                        .entry(counterparty.clone())
                        .or_insert_with(Vec::new)
                        .push(msg_to_send);

                    if let Some(chat) = self.chats.get_mut(&req.to_chat_id) {
                        if let Some(msg) = chat.messages.iter_mut().find(|m| m.id == forwarded_message.id) {
                            msg.status = MessageStatus::Failed;
                        }
                    }
                }
            }
        }

        Ok(forwarded_message)
    }

    #[http]
    async fn remove_reaction(&mut self, request_body: String) -> Result<String, String> {
        #[derive(Deserialize)]
        struct RemoveReactionRequest {
            message_id: String,
            emoji: String,
        }

        let req: RemoveReactionRequest = serde_json::from_str(&request_body)
            .map_err(|e| format!("Invalid request: {}", e))?;

        let user = our().node.clone();

        // Find and remove reaction from message
        for chat in self.chats.values_mut() {
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
    async fn create_chat_link(&mut self, request_body: String) -> Result<String, String> {
        #[derive(Deserialize)]
        struct CreateChatLinkRequest {
            single_use: bool,
        }

        let _req: CreateChatLinkRequest = serde_json::from_str(&request_body)
            .map_err(|e| format!("Invalid request: {}", e))?;

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
            chat_id: format!("browser:{}", key),
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
    async fn revoke_chat_key(&mut self, request_body: String) -> Result<String, String> {
        #[derive(Deserialize)]
        struct RevokeChatKeyRequest {
            key: String,
        }

        let req: RevokeChatKeyRequest = serde_json::from_str(&request_body)
            .map_err(|e| format!("Invalid request: {}", e))?;

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
    async fn update_settings(&mut self, request_body: String) -> Result<String, String> {
        let settings: Settings = serde_json::from_str(&request_body)
            .map_err(|e| format!("Invalid settings: {}", e))?;

        self.settings = settings;
        Ok("Settings updated".to_string())
    }

    #[http]
    async fn update_profile(&mut self, request_body: String) -> Result<String, String> {
        let profile: UserProfile = serde_json::from_str(&request_body)
            .map_err(|e| format!("Invalid profile: {}", e))?;

        self.profile = profile;
        Ok("Profile updated".to_string())
    }

    #[http]
    async fn upload_profile_picture(&mut self, request_body: String) -> Result<String, String> {
        #[derive(Deserialize)]
        struct UploadProfilePictureRequest {
            image_data: String, // Base64 encoded image
            mime_type: String,
        }

        let req: UploadProfilePictureRequest = serde_json::from_str(&request_body)
            .map_err(|e| format!("Invalid request: {}", e))?;

        // Validate mime type
        if !req.mime_type.starts_with("image/") {
            return Err("Invalid image type".to_string());
        }

        // Store the image data as a data URL
        let data_url = format!("data:{};base64,{}", req.mime_type, req.image_data);
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
    async fn upload_file(&mut self, request_body: String) -> Result<ChatMessage, String> {
        #[derive(Deserialize)]
        struct UploadFileRequest {
            chat_id: String,
            filename: String,
            mime_type: String,
            data: String, // Base64 encoded file data
            reply_to: Option<String>,
        }

        let req: UploadFileRequest = serde_json::from_str(&request_body)
            .map_err(|e| format!("Invalid request: {}", e))?;

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

        // Use data URL for file storage
        let file_url = format!("data:{};base64,{}", req.mime_type, req.data);

        let file_info = FileInfo {
            filename: req.filename.clone(),
            mime_type: req.mime_type,
            size: req.data.len() as u64,
            url: file_url,
        };

        let message = ChatMessage {
            id: message_id,
            sender: our().node.clone(),
            content: req.filename,
            timestamp,
            status: MessageStatus::Sending,
            reply_to: req.reply_to,
            reactions: Vec::new(),
            message_type,
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

        // Send to counterparty
        let counterparty = chat.counterparty.clone();
        let msg_to_send = message.clone();
        let body = serde_json::json!({"ReceiveMessage": msg_to_send});
        let body_bytes = serde_json::to_vec(&body).unwrap_or_default();

        let target = (counterparty.as_str(), OUR_PROCESS_ID);
        let request = Request::to(target)
            .body(body_bytes)
            .expects_response(5);

        match send::<Result<(), String>>(request).await {
            Ok(_) => {
                if let Some(chat) = self.chats.get_mut(&req.chat_id) {
                    if let Some(msg) = chat.messages.iter_mut().find(|m| m.id == message.id) {
                        msg.status = MessageStatus::Sent;
                    }
                }
            }
            Err(_) => {
                self.delivery_queue
                    .entry(counterparty.clone())
                    .or_insert_with(Vec::new)
                    .push(msg_to_send);
            }
        }

        // Broadcast to WebSocket connections
        for &channel_id in self.ws_connections.keys() {
            let msg = WsServerMessage::NewMessage(message.clone());
            send_ws_push(channel_id, WsMessageType::Text, LazyLoadBlob {
                mime: Some("application/json".to_string()),
                bytes: serde_json::to_string(&msg).unwrap().into_bytes(),
            });
        }

        Ok(message)
    }

    #[http]
    async fn send_voice_note(&mut self, request_body: String) -> Result<ChatMessage, String> {
        #[derive(Deserialize)]
        struct VoiceNoteRequest {
            chat_id: String,
            audio_data: String, // Base64 encoded audio
            duration: u64, // Duration in seconds
            reply_to: Option<String>,
        }

        let req: VoiceNoteRequest = serde_json::from_str(&request_body)
            .map_err(|e| format!("Invalid request: {}", e))?;

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

        // Send to counterparty
        let counterparty = chat.counterparty.clone();
        let msg_to_send = message.clone();
        let body = serde_json::json!({"ReceiveMessage": msg_to_send});
        let body_bytes = serde_json::to_vec(&body).unwrap_or_default();

        let target = (counterparty.as_str(), OUR_PROCESS_ID);
        let request = Request::to(target)
            .body(body_bytes)
            .expects_response(5);

        match send::<Result<(), String>>(request).await {
            Ok(_) => {
                if let Some(chat) = self.chats.get_mut(&req.chat_id) {
                    if let Some(msg) = chat.messages.iter_mut().find(|m| m.id == message.id) {
                        msg.status = MessageStatus::Sent;
                    }
                }
            }
            Err(_) => {
                self.delivery_queue
                    .entry(counterparty.clone())
                    .or_insert_with(Vec::new)
                    .push(msg_to_send);
            }
        }

        // Broadcast to WebSocket connections
        for &channel_id in self.ws_connections.keys() {
            let msg = WsServerMessage::NewMessage(message.clone());
            send_ws_push(channel_id, WsMessageType::Text, LazyLoadBlob {
                mime: Some("application/json".to_string()),
                bytes: serde_json::to_string(&msg).unwrap().into_bytes(),
            });
        }

        Ok(message)
    }

    // P2P MESSAGE RECEIVING

    #[remote]
    async fn receive_chat_creation(&mut self, counterparty: String) -> Result<(), String> {
        println!("receive_chat_creation: Got request from {}", counterparty);

        // Create the corresponding chat on this node
        let chat_id = format!("{}:{}", counterparty, our().node);
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Check if chat already exists
        if self.chats.contains_key(&chat_id) {
            println!("receive_chat_creation: Chat {} already exists", chat_id);
            return Ok(()); // Chat already exists, nothing to do
        }

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

        Ok(())
    }

    #[remote]
    async fn receive_message(&mut self, message: ChatMessage) -> Result<(), String> {
        // Find or create chat for this message
        let chat_id = format!("{}:{}", message.sender, our().node);
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
        updated_message.status = MessageStatus::Delivered;

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

        // Send acknowledgment back to sender
        let sender = message.sender.clone();
        let msg_id = message.id.clone();
        let ack_body = serde_json::json!({"MessageAck": msg_id});
        let ack_bytes = serde_json::to_vec(&ack_body).unwrap_or_default();

        let target = (sender.as_str(), OUR_PROCESS_ID);
        let request = Request::to(target)
            .body(ack_bytes)
            .expects_response(2);

        // Send acknowledgment asynchronously (don't wait for response)
        let _ = send::<Result<(), String>>(request).await;

        Ok(())
    }

    #[remote]
    async fn message_ack(&mut self, message_id: String) -> Result<(), String> {
        // Update message status to Delivered
        for chat in self.chats.values_mut() {
            if let Some(message) = chat.messages.iter_mut().find(|m| m.id == message_id) {
                message.status = MessageStatus::Delivered;

                // Notify WebSocket connections
                for &channel_id in self.ws_connections.keys() {
                    let msg = WsServerMessage::MessageAck { message_id: message_id.clone() };
                    send_ws_push(channel_id, WsMessageType::Text, LazyLoadBlob {
                        mime: Some("application/json".to_string()),
                        bytes: serde_json::to_string(&msg).unwrap().into_bytes(),
                    });
                }
                return Ok(());
            }
        }
        Err("Message not found".to_string())
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

    // SEARCH

    #[http]
    async fn search_chats(&self, request_body: String) -> Result<Vec<Chat>, String> {
        #[derive(Deserialize)]
        struct SearchRequest {
            query: String,
        }

        let req: SearchRequest = serde_json::from_str(&request_body)
            .map_err(|e| format!("Invalid request: {}", e))?;

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
            }
            WsMessageType::Text => {
                // Parse and handle client message
                if let Ok(payload) = String::from_utf8(blob.bytes.clone()) {
                    match serde_json::from_str::<WsClientMessage>(&payload) {
                        Ok(msg) => {
                            // Initialize connection if not already present
                            if !self.ws_connections.contains_key(&channel_id) && !self.browser_connections.values().any(|&ch| ch == channel_id) {
                                println!("WebSocket: New connection from channel {}", channel_id);
                                self.ws_connections.insert(channel_id, our().node.clone());
                                
                                // Send all existing chats to the new connection
                                for chat in self.chats.values() {
                                    let chat_update = WsServerMessage::ChatUpdate(chat.clone());
                                    send_ws_push(channel_id, WsMessageType::Text, LazyLoadBlob {
                                        mime: Some("application/json".to_string()),
                                        bytes: serde_json::to_string(&chat_update).unwrap().into_bytes(),
                                    });
                                }
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
impl AppState {
    async fn process_delivery_queue(&mut self) {
        // Process queued messages for each node
        let queue_snapshot = self.delivery_queue.clone();

        for (node, messages) in queue_snapshot {
            // Check if node is now online
            if self.online_nodes.contains(&node) {
                // Try to deliver all queued messages
                for msg in messages {
                    let body = serde_json::json!({"ReceiveMessage": msg});
                    let body_bytes = serde_json::to_vec(&body).unwrap_or_default();

                    let target = (node.as_str(), OUR_PROCESS_ID);
                    let request = Request::to(target)
                        .body(body_bytes)
                        .expects_response(5);

                    // Try to send
                    if let Ok(_) = send::<Result<(), String>>(request).await {
                        // Remove from queue if successful
                        if let Some(queue) = self.delivery_queue.get_mut(&node) {
                            queue.retain(|m| m.id != msg.id);
                        }
                    }
                }

                // Clean up empty queues
                if self.delivery_queue.get(&node).map_or(false, |q| q.is_empty()) {
                    self.delivery_queue.remove(&node);
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
                    status: MessageStatus::Sent,
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
                        self.delivery_queue.entry(counterparty)
                            .or_insert_with(Vec::new)
                            .push(message.clone());
                    }
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
                        message.status = MessageStatus::Delivered;
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
