use crate::hyperware::process::chat::{ChatMessage, MessageStatus, MessageType};
use crate::hyperware::process::tester::{
    Request as TesterRequest, Response as TesterResponse, RunRequest,
};
use hyperware_process_lib::{
    await_message, call_init, print_to_terminal, Address, ProcessId, Request, Response,
};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use std::{thread, time::Duration};

mod tester_lib;
mod crdt_core_tests;
mod membership_acl_tests;
mod group_threading_tests;
mod dm_extended_tests;
mod replication_admin_tests;

wit_bindgen::generate!({
    path: "../target/wit",
    world: "chat-test-ware-dot-hypr-v0",
    generate_unused_types: true,
    additional_derives: [PartialEq, serde::Deserialize, serde::Serialize, process_macros::SerdeJsonInto],
});

call_init!(init);
fn init(our: Address) {
    print_to_terminal(0, "begin");

    loop {
        handle_message(&our);
    }
}

fn handle_message(our: &Address) {
    let message = await_message()
        .unwrap_or_else(|e| fail_with(format!("failed to receive tester message: {e:?}")));

    if !message.is_request() {
        fail_with("expected tester request message");
    }

    let source = message.source();
    if our.node != source.node {
        fail_with(format!("rejecting foreign message from {:?}", source));
    }

    let TesterRequest::Run(RunRequest {
        input_node_names: node_names,
        ..
    }) = message
        .body()
        .try_into()
        .unwrap_or_else(|e| fail_with(format!("failed to decode tester run request: {e:?}")));

    print_to_terminal(0, "chat_test: start");

    if our.node != node_names[0] {
        Response::new()
            .body(TesterResponse::Run(Ok(())))
            .send()
            .unwrap_or_else(|e| fail_with(format!("failed to send tester ack: {e:?}")));
        return;
    }

    let chat_address = chat_process_address(&our.node);
    run_duplicate_message_test(&chat_address);
    run_pagination_timestamp_test(&chat_address, &our.node);

    if node_names.len() < 2 {
        fail_with("edit-message propagation test requires at least two nodes");
    }
    let remote_node = node_names[1].clone();
    run_edit_message_propagation_test(&our.node, &remote_node);
    run_counterparty_inference_test(&our.node, &remote_node);

    crdt_core_tests::run_group_crdt_flow_tests(&our.node, &remote_node);
    membership_acl_tests::run_membership_acl_tests(&our.node, &remote_node);
    group_threading_tests::run_group_threading_tests(&our.node);
    dm_extended_tests::run_dm_extended_tests(&our.node, &remote_node);
    replication_admin_tests::run_replication_admin_tests(&our.node, &remote_node);

    Response::new()
        .body(TesterResponse::Run(Ok(())))
        .send()
        .unwrap_or_else(|e| fail_with(format!("failed to send tester success: {e:?}")));
}

fn run_duplicate_message_test(chat_address: &Address) {
    let counterparty = "counterparty.chat-test".to_string();
    let message_id = "duplicate-message".to_string();

    let inbound_message = ChatMessage {
        id: message_id.clone(),
        sender: counterparty.clone(),
        content: "Hello from tests".to_string(),
        timestamp: 1,
        sequence: None,
        status: MessageStatus::Sent,
        reply_to: None,
        reactions: Vec::new(),
        message_type: MessageType::Text,
        file_info: None,
    };

    send_receive_message(chat_address, &inbound_message);
    send_receive_message(chat_address, &inbound_message);

    let chat_id = normalize_chat_id(&counterparty, &chat_address.node);
    let messages = fetch_messages(chat_address, &chat_id);
    let duplicate_count = messages.iter().filter(|m| m.id == message_id).count();

    if duplicate_count != 1 {
        fail_with(format!(
            "expected exactly one copy of message {message_id}, found {duplicate_count}"
        ));
    }
}

fn run_edit_message_propagation_test(local_node: &str, remote_node: &str) {
    let local_address = chat_process_address(local_node);
    let remote_address = chat_process_address(remote_node);
    let chat_id = normalize_chat_id(local_node, remote_node);

    create_chat(&local_address, remote_node);

    let original_content = "Original content";
    let edited_content = "Edited content";

    print_to_terminal(0, "1");
    let sent_message = send_chat_message(&local_address, &chat_id, original_content);
    print_to_terminal(0, "1.5");
    wait_for_remote_message(&remote_address, &chat_id, &sent_message.id);
    print_to_terminal(0, "2");

    edit_chat_message(&local_address, &chat_id, &sent_message.id, edited_content);
    print_to_terminal(0, "3");

    thread::sleep(Duration::from_millis(200));

    let remote_messages = fetch_messages(&remote_address, &chat_id);
    print_to_terminal(0, "4");

    let remote_message = remote_messages
        .iter()
        .find(|m| m.id == sent_message.id)
        .unwrap_or_else(|| {
            fail_with(format!(
                "remote node {remote_node} did not store message {}",
                sent_message.id
            ))
        });
    print_to_terminal(0, "5");

    if remote_message.content != edited_content {
        fail_with(format!(
            "expected remote message {} content to be '{edited_content}', found '{}'",
            sent_message.id, remote_message.content
        ));
    }
}

fn run_pagination_timestamp_test(chat_address: &Address, local_node: &str) {
    let counterparty = format!("pagination-peer.{}", local_node);
    let chat_id = normalize_chat_id(&counterparty, local_node);
    let timestamp = 1;

    for idx in 0..3 {
        let inbound_message = ChatMessage {
            id: format!("pagination-{idx}"),
            sender: counterparty.clone(),
            content: format!("Pagination message {idx}"),
            timestamp,
            sequence: None,
            status: MessageStatus::Sent,
            reply_to: None,
            reactions: Vec::new(),
            message_type: MessageType::Text,
            file_info: None,
        };
        send_receive_message(chat_address, &inbound_message);
    }

    let first_page =
        fetch_messages_with_before(chat_address, &chat_id, None, Some(2));
    if first_page.len() < 2 {
        fail_with("pagination test: expected at least two messages on the first page");
    }

    let cursor_message = first_page
        .last()
        .unwrap_or_else(|| fail_with("pagination test: expected messages on first page"));

    let older_messages = fetch_messages_with_before(
        chat_address,
        &chat_id,
        Some(cursor_message.timestamp),
        Some(2),
    );
    if older_messages.is_empty() {
        fail_with("pagination test: expected messages at cursor timestamp but received none");
    }
}

fn run_counterparty_inference_test(local_node: &str, remote_node: &str) {
    print_to_terminal(0, "running counterparty inference test");
    print_to_terminal(0, "local_node");
    print_to_terminal(0, local_node);
    print_to_terminal(0, "remote_node");
    print_to_terminal(0, remote_node);

    // We only care about the case where the remote node sorts after the local node,
    // since the remote will hit the fallback counterparty inference logic.
    if remote_node <= local_node {
        print_to_terminal(0, "remote does not sort after local; skipping");
        return;
    }

    let local_address = chat_process_address(local_node);
    let remote_address = chat_process_address(remote_node);
    let chat_id = normalize_chat_id(local_node, remote_node);

    print_to_terminal(0, "normalized chat id ");

    let _ = delete_chat_if_exists(&local_address, &chat_id);
    let _ = delete_chat_if_exists(&remote_address, &chat_id);

    print_to_terminal(0, "deleted chat");
    let sent_message =
        send_chat_message(&remote_address, &chat_id, "Counterparty inference message");

    print_to_terminal(0, "sent msg");
    wait_for_remote_message(&local_address, &chat_id, &sent_message.id);
    print_to_terminal(0, "received remote msg");
}

fn send_receive_message(address: &Address, message: &ChatMessage) {
    let payload = json!({"ReceiveMessage": message});
    let result: Result<(), String> = send_chat_rpc(address, payload);
    unwrap_chat_result(result);
}

fn fetch_messages(address: &Address, chat_id: &str) -> Vec<ChatMessage> {
    fetch_messages_with_before(address, chat_id, None, None)
}

fn fetch_messages_with_before(
    address: &Address,
    chat_id: &str,
    before_timestamp: Option<u64>,
    limit: Option<u64>,
) -> Vec<ChatMessage> {
    let payload = json!({
        "GetMessages": {
            "chat_id": chat_id,
            "before_timestamp": before_timestamp.map(Value::from).unwrap_or(Value::Null),
            "limit": limit
                .map(Value::from)
                .unwrap_or(Value::Null)
        }
    });
    let result: Result<Vec<ChatMessage>, String> = send_chat_rpc(address, payload);
    unwrap_chat_result(result)
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

fn send_chat_message(address: &Address, chat_id: &str, content: &str) -> ChatMessage {
    let payload = json!({
        "SendMessage": {
            "chat_id": chat_id,
            "content": content,
            "reply_to": Value::Null
        }
    });
    let result: Result<ChatMessage, String> = send_chat_rpc(address, payload);
    unwrap_chat_result(result)
}

fn delete_chat_if_exists(address: &Address, chat_id: &str) -> Result<String, String> {
    let payload = json!({
        "DeleteChat": {
            "chat_id": chat_id
        }
    });
    send_chat_rpc(address, payload)
}

fn edit_chat_message(address: &Address, chat_id: &str, message_id: &str, new_content: &str) {
    let payload = json!({
        "EditMessage": {
            "chat_id": chat_id,
            "message_id": message_id,
            "new_content": new_content
        }
    });
    let result: Result<String, String> = send_chat_rpc(address, payload);
    unwrap_chat_result(result);
}

fn create_chat(address: &Address, counterparty: &str) {
    let payload = json!({ "CreateChat": { "counterparty": counterparty } });
    let result: Result<Value, String> = send_chat_rpc(address, payload);
    unwrap_chat_result(result);
}

fn wait_for_remote_message(address: &Address, chat_id: &str, message_id: &str) {
    for _ in 0..60 {
        match fetch_messages_allow_missing(address, chat_id) {
            Ok(messages) => {
                if messages.iter().any(|m| m.id == message_id) {
                    return;
                }
            }
            Err(_) => {}
        }
        thread::sleep(Duration::from_millis(500));
    }
    fail_with(format!(
        "remote node {} never stored message {}",
        address.node, message_id
    ));
}

fn fetch_messages_allow_missing(
    address: &Address,
    chat_id: &str,
) -> Result<Vec<ChatMessage>, String> {
    let payload = json!({
        "GetMessages": {
            "chat_id": chat_id,
            "before_timestamp": Value::Null,
            "limit": Value::Null
        }
    });
    print_to_terminal(0, "DEBUGGING: send chat rpc GetMessages");
    send_chat_rpc(address, payload)
}

fn send_chat_rpc<T: DeserializeOwned>(address: &Address, payload: Value) -> Result<T, String> {
    print_to_terminal(
        0,
        format!(
            "chat_test: rpc -> {}@{} payload {}",
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
    let log = format!("chat_test: error: {message}");
    print_to_terminal(0, log.as_str());
    fail!(message);
}
