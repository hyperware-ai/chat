use crate::{
    safe_update_message_status, ChatMessage, ChatState, MessageStatus, MessageType,
    WsClientMessage, WsServerMessage,
};
use hyperware_process_lib::{
    http::server::{send_ws_push, WsMessageType},
    LazyLoadBlob,
};
use serde_json;

impl ChatState {
    pub(crate) fn handle_client_message(&mut self, channel_id: u32, msg: WsClientMessage) {
        match msg {
            WsClientMessage::SendMessage {
                chat_id,
                content,
                reply_to,
            } => {
                if let Err(err) =
                    self.send_message_internal(&chat_id, content, reply_to, Some(channel_id))
                {
                    crate::log_debug!("Failed to send message via WS: {}", err);
                }
            }
            WsClientMessage::Ack { message_id } => {
                // Update message status
                for chat in self.chats.values_mut() {
                    if let Some(message) = chat.messages.iter_mut().find(|m| m.id == message_id) {
                        message.status =
                            safe_update_message_status(&message.status, MessageStatus::Delivered);
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
                    self.broadcast_ws_message(&msg);
                }
            }
            WsClientMessage::Heartbeat => {
                let msg = WsServerMessage::Heartbeat;
                self.push_ws_message(channel_id, &msg);
            }
            _ => {
                // Other message types not handled in node-to-node
            }
        }
    }

    pub(crate) fn handle_browser_message(&mut self, channel_id: u32, msg: WsClientMessage) {
        match msg {
            WsClientMessage::AuthWithKey { chat_key } => {
                if let Some(key_data) = self.chat_keys.get(&chat_key) {
                    if !key_data.is_revoked {
                        // Store connection
                        self.browser_connections
                            .insert(chat_key.clone(), channel_id);

                        // Get chat history
                        let history = self
                            .chats
                            .get(&key_data.chat_id)
                            .map(|chat| chat.messages.clone())
                            .unwrap_or_default();

                        let msg = WsServerMessage::AuthSuccess {
                            chat_id: key_data.chat_id.clone(),
                            history,
                        };
                        self.push_ws_message(channel_id, &msg);
                    } else {
                        let msg = WsServerMessage::AuthFailed {
                            reason: "Chat key has been revoked".to_string(),
                        };
                        self.push_ws_message(channel_id, &msg);
                    }
                } else {
                    let msg = WsServerMessage::AuthFailed {
                        reason: "Invalid chat key".to_string(),
                    };
                    self.push_ws_message(channel_id, &msg);
                }
            }
            WsClientMessage::BrowserMessage { content } => {
                // Find chat key for this connection
                if let Some((chat_key, _)) = self
                    .browser_connections
                    .iter()
                    .find(|(_, &ch)| ch == channel_id)
                {
                    if let Some(key_data) = self.chat_keys.get(chat_key).cloned() {
                        let timestamp = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_secs();

                        let chat_id = key_data.chat_id.clone();
                        self.get_or_create_chat(
                            &chat_id,
                            timestamp,
                            Some(key_data.user_name.clone()),
                            None,
                        );

                        let mut message = ChatMessage {
                            id: format!("{}:{}", timestamp, rand::random::<u32>()),
                            sender: key_data.user_name.clone(),
                            content,
                            timestamp,
                            sequence: None,
                            status: MessageStatus::Sent,
                            reply_to: None,
                            reactions: Vec::new(),
                            message_type: MessageType::Text,
                            file_info: None,
                        };

                        self.assign_sequence_to_message(&chat_id, &mut message);

                        self.get_or_create_chat(
                            &chat_id,
                            timestamp,
                            Some(key_data.user_name.clone()),
                            None,
                        );
                        {
                            let chat = self.get_or_create_chat(
                                &chat_id,
                                timestamp,
                                Some(key_data.user_name.clone()),
                                None,
                            );
                            chat.messages.push(message.clone());
                            chat.last_activity = timestamp;
                            chat.unread_count += 1;
                        }

                        // Send message to all participants
                        let msg = WsServerMessage::NewMessage(message);
                        self.push_ws_message(channel_id, &msg);
                    }
                }
            }
            WsClientMessage::Heartbeat => {
                let msg = WsServerMessage::Heartbeat;
                self.push_ws_message(channel_id, &msg);
            }
            _ => {}
        }
    }

    pub(crate) fn push_ws_message(&self, channel_id: u32, message: &WsServerMessage) {
        match serde_json::to_vec(message) {
            Ok(bytes) => send_ws_push(
                channel_id,
                WsMessageType::Text,
                LazyLoadBlob {
                    mime: Some("application/json".to_string()),
                    bytes,
                },
            ),
            Err(err) => crate::log_debug!("Failed to serialize WS message: {:?}", err),
        }
    }

    pub(crate) fn broadcast_ws_message(&self, message: &WsServerMessage) {
        for &channel_id in self.ws_connections.keys() {
            self.push_ws_message(channel_id, message);
        }
    }
}
