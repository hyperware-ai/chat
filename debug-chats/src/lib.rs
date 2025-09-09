use hyperware_process_lib::{println, script, Address, Request};
use serde_json::Value;

wit_bindgen::generate!({
    path: "../target/wit",
    world: "process-v1",
});

const USAGE: &str = r#"\x1b[1mUsage:\x1b[0m
  debug-chats get_chats              - List all chats with summary
  debug-chats get_chat <chat_id>     - Show detailed messages for a specific chat

Examples:
  debug-chats get_chats
  debug-chats get_chat alice-hypr-bob-hypr
"#;

const CHAT_PROCESS_ID: (&str, &str, &str) = ("chat", "chat", "ware.hypr");

script!(init);
fn init(our: Address, args: String) -> String {
    if args.is_empty() {
        return USAGE.to_string();
    }

    let parts: Vec<&str> = args.split_whitespace().collect();
    if parts.is_empty() {
        return USAGE.to_string();
    }

    let chat_address = Address::new(our.node(), CHAT_PROCESS_ID);
    
    match parts[0] {
        "get_chats" => {
            get_chats(&chat_address)
        }
        "get_chat" => {
            if parts.len() < 2 {
                return format!("Error: get_chat requires a chat_id\n\n{}", USAGE);
            }
            let chat_id = parts[1..].join(" ");
            get_chat(&chat_address, &chat_id)
        }
        _ => {
            format!("Unknown command: {}\n\n{}", parts[0], USAGE)
        }
    }
}

fn get_chats(chat_address: &Address) -> String {
    let request = serde_json::json!({
        "GetChats": null
    });

    match Request::to(chat_address)
        .body(serde_json::to_vec(&request).unwrap_or_default())
        .send_and_await_response(10)
    {
        Ok(Ok(response_msg)) => {
            let response: Value = match serde_json::from_slice(response_msg.body()) {
                Ok(v) => v,
                Err(e) => return format!("Failed to parse response: {}", e),
            };

            // Extract chats from Ok response
            let chats = match response.get("Ok") {
                Some(chats_value) => chats_value,
                None => {
                    if let Some(err) = response.get("Err") {
                        return format!("Error from chat process: {}", err);
                    }
                    return format!("Unexpected response format: {}", response);
                }
            };

            // Parse as array of chats
            let chats_array = match chats.as_array() {
                Some(arr) => arr,
                None => return format!("Chats is not an array: {}", chats),
            };

            let mut output = String::new();
            output.push_str(&format!("\n=== Found {} chats ===\n", chats_array.len()));
            output.push_str(&"=".repeat(80));
            output.push_str("\n");

            for (i, chat) in chats_array.iter().enumerate() {
                let id = chat.get("id").and_then(|v| v.as_str()).unwrap_or("unknown");
                let counterparty = chat.get("counterparty").and_then(|v| v.as_str()).unwrap_or("unknown");
                let messages = chat.get("messages").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0);
                let last_activity = chat.get("last_activity").and_then(|v| v.as_u64()).unwrap_or(0);
                let unread = chat.get("unread_count").and_then(|v| v.as_u64()).unwrap_or(0);

                output.push_str(&format!("\n[Chat {}]\n", i + 1));
                output.push_str(&format!("  ID: {}\n", id));
                output.push_str(&format!("  Counterparty: {}\n", counterparty));
                output.push_str(&format!("  Messages: {}\n", messages));
                output.push_str(&format!("  Last Activity: {} ({})\n", 
                    last_activity,
                    format_time_ago(last_activity)
                ));
                output.push_str(&format!("  Unread: {}\n", unread));

                // Show last 2 messages preview
                if let Some(messages_array) = chat.get("messages").and_then(|v| v.as_array()) {
                    if !messages_array.is_empty() {
                        output.push_str("\n  Recent messages:\n");
                        let start = if messages_array.len() > 2 { messages_array.len() - 2 } else { 0 };
                        
                        for msg in &messages_array[start..] {
                            let sender = msg.get("sender").and_then(|v| v.as_str()).unwrap_or("?");
                            let content = msg.get("content").and_then(|v| v.as_str()).unwrap_or("");
                            let msg_id = msg.get("id").and_then(|v| v.as_str()).unwrap_or("?");
                            let timestamp = msg.get("timestamp").and_then(|v| v.as_u64()).unwrap_or(0);
                            
                            let content_preview = if content.len() > 50 {
                                format!("{}...", &content[..50])
                            } else {
                                content.to_string()
                            };
                            
                            output.push_str(&format!("    [{} ago] {}: {}\n", 
                                format_time_ago(timestamp),
                                sender,
                                content_preview
                            ));
                        }
                    }
                }
            }
            
            output.push_str(&format!("\n{}\n", "=".repeat(80)));
            output
        }
        Ok(Err(e)) => format!("Request failed: {}", e),
        Err(e) => format!("Failed to send request: {:?}", e),
    }
}

fn get_chat(chat_address: &Address, chat_id: &str) -> String {
    let request = serde_json::json!({
        "GetChat": {
            "chat_id": chat_id
        }
    });

    match Request::to(chat_address)
        .body(serde_json::to_vec(&request).unwrap_or_default())
        .send_and_await_response(10)
    {
        Ok(Ok(response_msg)) => {
            let response: Value = match serde_json::from_slice(response_msg.body()) {
                Ok(v) => v,
                Err(e) => return format!("Failed to parse response: {}", e),
            };

            // Extract chat from Ok response
            let chat = match response.get("Ok") {
                Some(chat_value) => chat_value,
                None => {
                    if let Some(err) = response.get("Err") {
                        return format!("Error from chat process: {}", err);
                    }
                    return format!("Unexpected response format: {}", response);
                }
            };

            let mut output = String::new();
            output.push_str(&format!("\n=== Chat Details: {} ===\n", chat_id));
            output.push_str(&"=".repeat(80));
            output.push_str("\n");

            let counterparty = chat.get("counterparty").and_then(|v| v.as_str()).unwrap_or("unknown");
            let last_activity = chat.get("last_activity").and_then(|v| v.as_u64()).unwrap_or(0);
            let unread = chat.get("unread_count").and_then(|v| v.as_u64()).unwrap_or(0);
            let is_blocked = chat.get("is_blocked").and_then(|v| v.as_bool()).unwrap_or(false);
            let notify = chat.get("notify").and_then(|v| v.as_bool()).unwrap_or(true);

            output.push_str(&format!("Counterparty: {}\n", counterparty));
            output.push_str(&format!("Last Activity: {} ({})\n", last_activity, format_time_ago(last_activity)));
            output.push_str(&format!("Unread: {}\n", unread));
            output.push_str(&format!("Blocked: {}\n", is_blocked));
            output.push_str(&format!("Notifications: {}\n", notify));

            // Show all messages
            if let Some(messages_array) = chat.get("messages").and_then(|v| v.as_array()) {
                output.push_str(&format!("\n=== {} Messages ===\n", messages_array.len()));
                
                for (i, msg) in messages_array.iter().enumerate() {
                    output.push_str(&format!("\n[Message {} of {}]\n", i + 1, messages_array.len()));
                    
                    let msg_id = msg.get("id").and_then(|v| v.as_str()).unwrap_or("?");
                    let sender = msg.get("sender").and_then(|v| v.as_str()).unwrap_or("?");
                    let content = msg.get("content").and_then(|v| v.as_str()).unwrap_or("");
                    let timestamp = msg.get("timestamp").and_then(|v| v.as_u64()).unwrap_or(0);
                    let status = msg.get("status").and_then(|v| v.as_str()).unwrap_or("?");
                    let msg_type = msg.get("message_type").and_then(|v| v.as_str()).unwrap_or("Text");
                    
                    output.push_str(&format!("  ID: {}\n", msg_id));
                    output.push_str(&format!("  Sender: {}\n", sender));
                    output.push_str(&format!("  Time: {} ({})\n", timestamp, format_time_ago(timestamp)));
                    output.push_str(&format!("  Status: {}\n", status));
                    output.push_str(&format!("  Type: {}\n", msg_type));
                    
                    if let Some(reply_to) = msg.get("reply_to").and_then(|v| v.as_str()) {
                        output.push_str(&format!("  Reply To: {}\n", reply_to));
                    }
                    
                    if let Some(reactions) = msg.get("reactions").and_then(|v| v.as_array()) {
                        if !reactions.is_empty() {
                            output.push_str("  Reactions: ");
                            for reaction in reactions {
                                let emoji = reaction.get("emoji").and_then(|v| v.as_str()).unwrap_or("?");
                                let user = reaction.get("user").and_then(|v| v.as_str()).unwrap_or("?");
                                output.push_str(&format!("{} ({}), ", emoji, user));
                            }
                            output.push_str("\n");
                        }
                    }
                    
                    if let Some(file_info) = msg.get("file_info").and_then(|v| v.as_object()) {
                        let filename = file_info.get("filename").and_then(|v| v.as_str()).unwrap_or("?");
                        let size = file_info.get("size").and_then(|v| v.as_u64()).unwrap_or(0);
                        let mime = file_info.get("mime_type").and_then(|v| v.as_str()).unwrap_or("?");
                        output.push_str(&format!("  File: {} ({} bytes, {})\n", filename, size, mime));
                    }
                    
                    output.push_str(&format!("  Content: {}\n", content));
                }
            } else {
                output.push_str("\n=== No messages ===\n");
            }
            
            output.push_str(&format!("\n{}\n", "=".repeat(80)));
            output
        }
        Ok(Err(e)) => format!("Request failed: {}", e),
        Err(e) => format!("Failed to send request: {:?}", e),
    }
}

fn format_time_ago(timestamp: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    
    if timestamp > now {
        return "future".to_string();
    }
    
    let diff = now - timestamp;
    
    if diff < 60 {
        format!("{}s", diff)
    } else if diff < 3600 {
        format!("{}m", diff / 60)
    } else if diff < 86400 {
        format!("{}h", diff / 3600)
    } else {
        format!("{}d", diff / 86400)
    }
}