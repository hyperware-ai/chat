use hyperware_process_lib::{print_to_terminal, Address, ProcessId, Request};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};

pub fn run_group_crdt_flow_tests(local_node: &str, remote_node: &str) {
    print_to_terminal(0, "crdt_core: group crdt flow test start");
    let local = chat_process_address(local_node);
    let remote = chat_process_address(remote_node);

    let create_group_res = create_group(&local, "Split CRDT Group");
    let group_id = create_group_res.group_id.clone();

    let child = create_group_thread(&local, &group_id, None);
    assert_thread_suffix(&child.thread_id, 1);

    let msg = send_group_message(&local, &group_id, None, "hello world");
    assert_message_suffix(&msg.message.message_id, 0);

    let sv_err = crdt_group_state_vector(&remote, &group_id)
        .err()
        .unwrap_or_else(|| fail_with("expected state vector error before bootstrap"));
    if !sv_err.contains("pending bootstrap") {
        fail_with(format!(
            "expected pending bootstrap error for state vector, got: {sv_err}"
        ));
    }

    let bad_sv_err = crdt_group_update(&local, &group_id, Some("!!bad!!".to_string()))
        .err()
        .unwrap_or_else(|| fail_with("expected error for bad group state vector"));
    if !bad_sv_err.contains("Invalid state vector payload") {
        fail_with(format!(
            "expected invalid state vector payload error, got: {bad_sv_err}"
        ));
    }

    let bad_apply_err = crdt_group_apply_update(&remote, &group_id, "!!bad!!")
        .err()
        .unwrap_or_else(|| fail_with("expected error for bad group update payload"));
    if !bad_apply_err.contains("Invalid update payload") {
        fail_with(format!(
            "expected invalid update payload error, got: {bad_apply_err}"
        ));
    }

    let full = crdt_group_update(&local, &group_id, None)
        .unwrap_or_else(|e| fail_with(format!("crdt_group_update failed: {e}")));
    if full.update_payload.is_empty() {
        fail_with("expected non-empty full group update payload");
    }

    let applied = crdt_group_apply_update(&remote, &group_id, &full.update_payload)
        .unwrap_or_else(|e| fail_with(format!("crdt_group_apply_update failed: {e}")));
    if !applied.applied {
        fail_with("expected crdt_group_apply_update.applied = true");
    }

    let remote_sv = crdt_group_state_vector(&remote, &group_id)
        .unwrap_or_else(|e| fail_with(format!("state vector after bootstrap failed: {e}")));

    let local_child = create_group_thread(&local, &group_id, None);
    assert_thread_suffix(&local_child.thread_id, 2);

    let incremental = crdt_group_update(&local, &group_id, Some(remote_sv.state_vector))
        .unwrap_or_else(|e| fail_with(format!("incremental group update failed: {e}")));
    if incremental.update_payload.is_empty() {
        fail_with("expected non-empty incremental update payload");
    }

    let applied_inc = crdt_group_apply_update(&remote, &group_id, &incremental.update_payload)
        .unwrap_or_else(|e| fail_with(format!("apply incremental update failed: {e}")));
    if !applied_inc.applied {
        fail_with("expected incremental apply to succeed");
    }

    let new_child = create_group_thread(&remote, &group_id, None);
    assert_thread_suffix(&new_child.thread_id, 3);

    let new_msg = send_group_message(&remote, &group_id, None, "from remote");
    assert_message_suffix(&new_msg.message.message_id, 1);

    print_to_terminal(0, "crdt_core: group crdt flow test done");
}

// ---------- Local helpers ----------

#[derive(Deserialize)]
struct CrdtStateVectorRes {
    state_vector: String,
}

#[derive(Deserialize)]
struct CrdtUpdateRes {
    doc_id: String,
    update_payload: String,
}

#[derive(Deserialize)]
struct CrdtApplyRes {
    applied: bool,
}

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
struct MessageMetaLite {
    message_id: String,
}
fn crdt_group_state_vector(
    address: &Address,
    group_id: &str,
) -> Result<CrdtStateVectorRes, String> {
    let payload = json!({
        "CrdtGroupStateVector": { "group_id": group_id }
    });
    send_chat_rpc(address, payload)
}

fn crdt_group_update(
    address: &Address,
    group_id: &str,
    state_vector: Option<String>,
) -> Result<CrdtUpdateRes, String> {
    let sv_value = state_vector.map(Value::from).unwrap_or(Value::Null);
    let payload = json!({
        "CrdtGroupUpdate": {
            "group_id": group_id,
            "state_vector": sv_value
        }
    });
    send_chat_rpc(address, payload)
}

fn crdt_group_apply_update(
    address: &Address,
    group_id: &str,
    update_payload: &str,
) -> Result<CrdtApplyRes, String> {
    let payload = json!({
        "CrdtGroupApplyUpdate": {
            "group_id": group_id,
            "update_payload": update_payload
        }
    });
    send_chat_rpc(address, payload)
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

fn create_group_thread(
    address: &Address,
    group_id: &str,
    parent_thread_id: Option<&str>,
) -> CreateGroupThreadRes {
    let parent = parent_thread_id.map(Value::from).unwrap_or(Value::Null);
    let payload = json!({
        "CreateGroupThread": {
            "group_id": group_id,
            "parent_thread_id": parent,
            "title": Value::Null
        }
    });
    let result: Result<CreateGroupThreadRes, String> = send_chat_rpc(address, payload);
    unwrap_chat_result(result)
}

fn send_group_message(
    address: &Address,
    group_id: &str,
    thread_id: Option<&str>,
    content: &str,
) -> SendGroupMessageRes {
    let thread = thread_id.map(Value::from).unwrap_or(Value::Null);
    let payload = json!({
        "SendGroupMessage": {
            "group_id": group_id,
            "thread_id": thread,
            "content": content,
            "message_type": "Text",
            "reply_to": Value::Null,
            "attachments": []
        }
    });
    let result: Result<SendGroupMessageRes, String> = send_chat_rpc(address, payload);
    unwrap_chat_result(result)
}

fn assert_thread_suffix(thread_id: &str, expected: u64) {
    match thread_id.rsplit(':').next().and_then(|s| s.parse::<u64>().ok()) {
        Some(n) if n == expected => {}
        other => fail_with(format!(
            "unexpected thread id suffix for {thread_id}, expected {expected}, got {:?}",
            other
        )),
    }
}

fn assert_message_suffix(message_id: &str, expected: u64) {
    match message_id.rsplit(':').next().and_then(|s| s.parse::<u64>().ok()) {
        Some(n) if n == expected => {}
        other => fail_with(format!(
            "unexpected message id suffix for {message_id}, expected {expected}, got {:?}",
            other
        )),
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
            "crdt_core: rpc -> {}@{} payload {}",
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

    serde_json::from_slice(response.body())
        .unwrap_or_else(|e| fail_with(format!("failed to decode chat response: {e}")))
}

fn unwrap_chat_result<T>(result: Result<T, String>) -> T {
    match result {
        Ok(value) => value,
        Err(err) => fail_with(format!("chat RPC returned error: {err}")),
    }
}

fn fail_with(message: impl Into<String>) -> ! {
    let message = message.into();
    let log = format!("crdt_core: error: {message}");
    print_to_terminal(0, log.as_str());
    crate::fail!(message);
}
