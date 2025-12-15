use hyperware_process_lib::{print_to_terminal, Address, ProcessId, Request};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

use crate::fail;

pub fn run_group_message_actions_tests(local_node: &str) {
    print_to_terminal(0, "group_message_actions: start");
    let addr = chat_process_address(local_node);

    let group_id = create_group(&addr, "Actions Group").group_id;
    let root_thread_id = get_root_thread_id(&addr, &group_id);

    // Send a message
    let msg = send_group_message(&addr, &group_id, Some(&root_thread_id), "hello actions").message;
    let message_id = msg.message_id.clone();

    // Edit
    let edited = edit_group_message(&addr, &group_id, &message_id, "edited body").message;
    assert_eq!(
        edited.body, "edited body",
        "edited body should be returned from edit response"
    );

    // Add reaction
    add_group_reaction(&addr, &group_id, &message_id, "👍");
    let group_after_reaction = get_group(&addr, &group_id).group.unwrap_or_else(|| fail_with("missing group after reaction"));
    let reactions = group_after_reaction
        .messages
        .get(&message_id)
        .map(|m| m.reactions.clone())
        .unwrap_or_else(|| fail_with("message missing after reaction"));
    if reactions.len() != 1 || reactions[0].emoji != "👍" || reactions[0].node_id != local_node {
        fail_with("reaction not stored as expected");
    }

    // Remove reaction
    remove_group_reaction(&addr, &group_id, &message_id, "👍");
    let group_after_remove = get_group(&addr, &group_id).group.unwrap_or_else(|| fail_with("missing group after reaction removal"));
    let reactions_after = group_after_remove
        .messages
        .get(&message_id)
        .map(|m| m.reactions.clone())
        .unwrap_or_else(|| fail_with("message missing after reaction removal"));
    if !reactions_after.is_empty() {
        fail_with("reaction was not removed");
    }

    // Delete message
    delete_group_message(&addr, &group_id, &message_id);
    let group_after_delete = get_group(&addr, &group_id).group.unwrap_or_else(|| fail_with("missing group after delete"));
    if group_after_delete.messages.contains_key(&message_id) {
        fail_with("message still present after delete");
    }
    if let Some(thread) = group_after_delete.threads.get(&root_thread_id) {
        if thread.summary.message_count != 0 {
            fail_with("thread summary not decremented after delete");
        }
    }

    print_to_terminal(0, "group_message_actions: done");
}

// ---------- RPC helpers ----------

#[derive(Deserialize)]
struct CreateGroupRes {
    group_id: String,
}

#[derive(Deserialize)]
struct CreateGroupThreadRes {
    thread_id: String,
}

#[derive(Deserialize)]
struct SendGroupMessageRes {
    message: MessageMetaLite,
}

#[derive(Deserialize)]
struct GetGroupRes {
    group: Option<GroupLite>,
}

#[derive(Deserialize, Clone)]
struct GroupLite {
    metadata: Option<GroupMetadataLite>,
    threads: HashMap<String, ThreadLite>,
    messages: HashMap<String, MessageMetaLite>,
}

#[derive(Deserialize, Clone)]
struct GroupMetadataLite {
    root_thread_id: String,
}

#[derive(Deserialize, Clone)]
struct ThreadLite {
    summary: ThreadSummaryLite,
}

#[derive(Deserialize, Clone)]
struct ThreadSummaryLite {
    message_count: u64,
}

#[derive(Deserialize, Clone)]
struct MessageMetaLite {
    message_id: String,
    body: String,
    reactions: Vec<MessageReactionLite>,
}

#[derive(Deserialize, Clone)]
struct MessageReactionLite {
    node_id: String,
    emoji: String,
}

fn create_group(address: &Address, name: &str) -> CreateGroupRes {
    let payload = json!({
        "CreateGroup": {
            "group_id": Value::Null,
            "name": name,
            "description": Value::Null,
            "avatar": Value::Null,
            "visibility": Value::Null,
            "default_role_label": Value::Null,
            "membership_rules": [],
            "root_thread_title": Value::Null
        }
    });
    let result: Result<CreateGroupRes, String> = send_chat_rpc(address, payload);
    unwrap_chat_result(result)
}

fn get_group(address: &Address, group_id: &str) -> GetGroupRes {
    let payload = json!({
        "GetGroup": { "group_id": group_id }
    });
    let result: Result<GetGroupRes, String> = send_chat_rpc(address, payload);
    unwrap_chat_result(result)
}

fn send_group_message(
    address: &Address,
    group_id: &str,
    thread_id: Option<&str>,
    content: &str,
) -> SendGroupMessageRes {
    let payload = json!({
        "SendGroupMessage": {
            "group_id": group_id,
            "thread_id": thread_id.map(Value::from).unwrap_or(Value::Null),
            "content": content,
            "message_type": "Text",
            "reply_to": Value::Null,
            "attachments": []
        }
    });
    let result: Result<SendGroupMessageRes, String> = send_chat_rpc(address, payload);
    unwrap_chat_result(result)
}

fn edit_group_message(
    address: &Address,
    group_id: &str,
    message_id: &str,
    new_content: &str,
) -> SendGroupMessageRes {
    let payload = json!({
        "EditGroupMessage": {
            "group_id": group_id,
            "message_id": message_id,
            "new_content": new_content
        }
    });
    let result: Result<SendGroupMessageRes, String> = send_chat_rpc(address, payload);
    unwrap_chat_result(result)
}

fn delete_group_message(address: &Address, group_id: &str, message_id: &str) {
    let payload = json!({
        "DeleteGroupMessage": {
            "group_id": group_id,
            "message_id": message_id,
            "delete_for_both": Value::Null
        }
    });
    let result: Result<String, String> = send_chat_rpc(address, payload);
    unwrap_chat_result(result);
}

fn add_group_reaction(address: &Address, group_id: &str, message_id: &str, emoji: &str) {
    let payload = json!({
        "AddGroupReaction": {
            "group_id": group_id,
            "message_id": message_id,
            "emoji": emoji
        }
    });
    let result: Result<String, String> = send_chat_rpc(address, payload);
    unwrap_chat_result(result);
}

fn remove_group_reaction(address: &Address, group_id: &str, message_id: &str, emoji: &str) {
    let payload = json!({
        "RemoveGroupReaction": {
            "group_id": group_id,
            "message_id": message_id,
            "emoji": emoji
        }
    });
    let result: Result<String, String> = send_chat_rpc(address, payload);
    unwrap_chat_result(result);
}

fn get_root_thread_id(address: &Address, group_id: &str) -> String {
    let group = get_group(address, group_id)
        .group
        .unwrap_or_else(|| fail_with("group missing after create"));
    group
        .metadata
        .as_ref()
        .map(|m| m.root_thread_id.clone())
        .unwrap_or_else(|| fail_with("root thread id missing"))
}

fn send_chat_rpc<T: serde::de::DeserializeOwned>(
    address: &Address,
    payload: serde_json::Value,
) -> Result<T, String> {
    print_to_terminal(
        0,
        format!(
            "group_message_actions: rpc -> {}@{} payload {}",
            address.process,
            address.node,
            payload
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
    let log = format!("group_message_actions: error: {message}");
    print_to_terminal(0, log.as_str());
    fail!(message);
}

fn chat_process_address(node: &str) -> Address {
    Address {
        node: node.to_string(),
        process: ProcessId::new(Some("chat"), "chat", "ware.hypr"),
    }
}
