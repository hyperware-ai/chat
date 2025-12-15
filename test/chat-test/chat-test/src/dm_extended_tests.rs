use hyperware_process_lib::{print_to_terminal, Address, ProcessId, Request};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};
use std::thread;
use std::time::Duration;

use crate::fail;

pub fn run_dm_extended_tests(local_node: &str, remote_node: &str) {
    print_to_terminal(0, "dm_extended: start");
    let local = chat_process_address(local_node);
    let remote = chat_process_address(remote_node);
    let chat_id = normalize_chat_id(local_node, remote_node);

    // Ensure clean chat on both sides.
    let _ = delete_chat_if_exists(&local, &chat_id);
    let _ = delete_chat_if_exists(&remote, &chat_id);

    // Create chat and send a message from local to remote.
    create_chat(&local, remote_node);
    let sent = send_chat_message(&local, &chat_id, "dm_extended: hello");
    wait_for_message(&remote, &chat_id, &sent.id);

    // Remote adds a reaction.
    add_reaction(&remote, &chat_id, &sent.id, "👍");
    thread::sleep(Duration::from_millis(100));
    let messages = fetch_messages(&local, &chat_id);
    let msg = messages
        .iter()
        .find(|m| m.id == sent.id)
        .cloned()
        .unwrap_or_else(|| fail_with("message missing after reaction"));
    assert!(
        msg.reactions.iter().any(|r| r.user == remote_node && r.emoji == "👍"),
        "reaction from remote not present"
    );

    // Remove reaction.
    remove_reaction(&remote, &chat_id, &sent.id, "👍");
    thread::sleep(Duration::from_millis(100));
    let messages = fetch_messages(&local, &chat_id);
    let msg = messages
        .iter()
        .find(|m| m.id == sent.id)
        .cloned()
        .unwrap_or_else(|| fail_with("message missing after reaction removal"));
    assert!(
        msg.reactions.is_empty(),
        "reaction list should be empty after removal"
    );

    // Delete message for both sides.
    delete_message(&local, &chat_id, &sent.id, true);
    thread::sleep(Duration::from_millis(100));
    let local_msgs = fetch_messages(&local, &chat_id);
    if local_msgs.iter().any(|m| m.id == sent.id) {
        fail_with("local still has message after delete");
    }
    let remote_msgs = fetch_messages(&remote, &chat_id);
    if remote_msgs.iter().any(|m| m.id == sent.id) {
        fail_with("remote still has message after delete_for_both");
    }

    print_to_terminal(0, "dm_extended: done");
}

// ---------- RPC helpers ----------

#[derive(Deserialize, Clone)]
struct ChatMessageLite {
    id: String,
    sender: String,
    content: String,
    timestamp: u64,
    reactions: Vec<MessageReactionLite>,
}

#[derive(Deserialize, Clone)]
struct MessageReactionLite {
    emoji: String,
    user: String,
    timestamp: u64,
}

fn send_chat_message(address: &Address, chat_id: &str, content: &str) -> ChatMessageLite {
    let payload = json!({
        "SendMessage": {
            "chat_id": chat_id,
            "content": content,
            "reply_to": Value::Null
        }
    });
    let result: Result<ChatMessageLite, String> = send_chat_rpc(address, payload);
    unwrap_chat_result(result)
}

fn add_reaction(address: &Address, chat_id: &str, message_id: &str, emoji: &str) {
    let payload = json!({
        "AddReaction": {
            "chat_id": chat_id,
            "message_id": message_id,
            "emoji": emoji
        }
    });
    let result: Result<String, String> = send_chat_rpc(address, payload);
    unwrap_chat_result(result);
}

fn remove_reaction(address: &Address, chat_id: &str, message_id: &str, emoji: &str) {
    let payload = json!({
        "RemoveReaction": {
            "chat_id": chat_id,
            "message_id": message_id,
            "emoji": emoji
        }
    });
    let result: Result<String, String> = send_chat_rpc(address, payload);
    unwrap_chat_result(result);
}

fn delete_message(
    address: &Address,
    chat_id: &str,
    message_id: &str,
    delete_for_both: bool,
) {
    let payload = json!({
        "DeleteMessage": {
            "chat_id": chat_id,
            "message_id": message_id,
            "delete_for_both": delete_for_both
        }
    });
    let result: Result<String, String> = send_chat_rpc(address, payload);
    unwrap_chat_result(result);
}

fn fetch_messages(address: &Address, chat_id: &str) -> Vec<ChatMessageLite> {
    let payload = json!({
        "GetMessages": {
            "chat_id": chat_id,
            "before_timestamp": Value::Null,
            "limit": Value::Null
        }
    });
    let result: Result<Vec<ChatMessageLite>, String> = send_chat_rpc(address, payload);
    unwrap_chat_result(result)
}

fn wait_for_message(address: &Address, chat_id: &str, message_id: &str) {
    for _ in 0..60 {
        let msgs = fetch_messages_allow_missing(address, chat_id);
        if let Ok(messages) = msgs {
            if messages.iter().any(|m| m.id == message_id) {
                return;
            }
        }
        thread::sleep(Duration::from_millis(500));
    }
    fail_with("message did not arrive on remote");
}

fn fetch_messages_allow_missing(
    address: &Address,
    chat_id: &str,
) -> Result<Vec<ChatMessageLite>, String> {
    let payload = json!({
        "GetMessages": {
            "chat_id": chat_id,
            "before_timestamp": Value::Null,
            "limit": Value::Null
        }
    });
    send_chat_rpc(address, payload)
}

fn create_chat(address: &Address, counterparty: &str) {
    let payload = json!({ "CreateChat": { "counterparty": counterparty } });
    let result: Result<Value, String> = send_chat_rpc(address, payload);
    unwrap_chat_result(result);
}

fn delete_chat_if_exists(address: &Address, chat_id: &str) -> Result<String, String> {
    let payload = json!({
        "DeleteChat": {
            "chat_id": chat_id
        }
    });
    send_chat_rpc(address, payload)
}

fn normalize_chat_id(a: &str, b: &str) -> String {
    if a < b {
        format!("{a}:{b}")
    } else {
        format!("{b}:{a}")
    }
}

fn chat_process_address(node: &str) -> Address {
    Address {
        node: node.to_string(),
        process: ProcessId::new(Some("chat"), "chat", "ware.hypr"),
    }
}

fn send_chat_rpc<T: DeserializeOwned>(address: &Address, payload: Value) -> Result<T, String> {
    print_to_terminal(
        0,
        format!(
            "dm_extended: rpc -> {}@{} payload {}",
            address.process, address.node, payload
        )
        .as_str(),
    );
    let body = serde_json::to_vec(&payload)
        .unwrap_or_else(|e| fail_with(format!("failed to encode chat payload: {e}")));

    let response = Request::to(address.clone())
        .body(body)
        .send_and_await_response(15)
        .unwrap_or_else(|e| fail_with(format!("failed to send chat request: {e:?}")))
        .unwrap_or_else(|_| fail_with("chat request returned no response"));

    if response.is_request() {
        fail_with("chat request returned a request instead of a response");
    }

    let rpc_result: Result<T, String> = serde_json::from_slice(response.body())
        .unwrap_or_else(|e| fail_with(format!("failed to decode chat response: {e}")));

    rpc_result
}

fn unwrap_chat_result<T>(result: Result<T, String>) -> T {
    match result {
        Ok(value) => value,
        Err(err) => fail_with(format!("chat RPC returned error: {err}")),
    }
}

fn fail_with(message: impl Into<String>) -> ! {
    let message = message.into();
    let log = format!("dm_extended: error: {message}");
    print_to_terminal(0, log.as_str());
    fail!(message);
}
