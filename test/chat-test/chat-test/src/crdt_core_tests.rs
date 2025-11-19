use hyperware_process_lib::{print_to_terminal, Address, ProcessId, Request, Response};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};

// ---------- Public entry points ----------

pub fn run_crdt_http_endpoint_tests(chat_address: &Address) {
    print_to_terminal(0, "crdt_core: http endpoints test start");

    // 1) Get initial state vector
    let sv = get_crdt_state_vector(chat_address);

    // 2) Update with no state vector -> expect non-empty payload
    let upd_no_sv = crdt_update(chat_address, None).unwrap_or_else(|e| fail_with(format!(
        "crdt_update without SV returned error: {e}"
    )));
    if upd_no_sv.update_payload.is_empty() {
        fail_with("crdt_update without SV should return non-empty payload");
    }

    // 3) Update with state vector -> expect empty payload (no new changes)
    let upd_with_sv = crdt_update(chat_address, Some(sv.state_vector.clone()))
        .unwrap_or_else(|e| fail_with(format!("crdt_update with SV returned error: {e}")));
    if !upd_with_sv.update_payload.is_empty() {
        print_to_terminal(0, &format!(
            "WARN: expected empty update with SV; got {} bytes",
            upd_with_sv.update_payload.len()
        ));
    }

    // 4) Bad base64 for state vector -> error
    let bad_sv_err = crdt_update(chat_address, Some("!!not-base64!!".to_string()))
        .err()
        .unwrap_or_else(|| fail_with("expected crdt_update error for bad SV"));
    if !bad_sv_err.contains("Invalid state vector payload") {
        fail_with(format!(
            "expected 'Invalid state vector payload' in error, got: {bad_sv_err}"
        ));
    }

    // 5) Bad base64 for apply update -> error
    let bad_apply_err = crdt_apply_update(chat_address, "!!not-base64!!").err().unwrap_or_else(|| {
        fail_with("expected crdt_apply_update error for bad update payload")
    });
    if !bad_apply_err.contains("Invalid update payload") {
        fail_with(format!(
            "expected 'Invalid update payload' in error, got: {bad_apply_err}"
        ));
    }

    // 6) Apply a valid update payload -> applied = true
    let good_update = upd_no_sv.update_payload;
    let applied = crdt_apply_update(chat_address, &good_update)
        .unwrap_or_else(|e| fail_with(format!("crdt_apply_update failed: {e}")));
    if !applied.applied {
        fail_with("expected crdt_apply_update.applied = true");
    }

    print_to_terminal(0, "crdt_core: http endpoints test done");
}

pub fn run_crdt_replication_allocator_tests(local_node: &str, remote_node: &str) {
    print_to_terminal(0, "crdt_core: replication/allocator test start");
    let local = chat_process_address(local_node);
    let remote = chat_process_address(remote_node);

    // Create a new group on local; expect root thread counter = 0
    let create_group_res = create_group(&local, "CRDT Core Group");
    let group_id = create_group_res.group_id.clone();

    // let groups_summaries = 

    // Create one child thread on local to advance next_thread to 1
    let child = create_group_thread(&local, &group_id, None);
    assert_thread_suffix(&child.thread_id, 1);

    // Send one message on local to advance next_message to 1
    let msg = send_group_message(&local, &group_id, None, "hello world");
    assert_message_suffix(&msg.message.message_id, 0); // first message is :msg:0

    // Get remote SV then fetch update from local since that SV
    let remote_sv = get_crdt_state_vector(&remote);
    let update = crdt_update(&local, Some(remote_sv.state_vector)).unwrap_or_else(|e| {
        fail_with(format!("crdt_update from local failed: {e}"))
    });
    if update.update_payload.is_empty() {
        fail_with("expected non-empty update payload for replication");
    }

    // Apply update on remote
    let applied = crdt_apply_update(&remote, &update.update_payload).unwrap_or_else(|e| {
        fail_with(format!("crdt_apply_update on remote failed: {e}"))
    });
    if !applied.applied {
        fail_with("expected crdt_apply_update.applied = true on remote");
    }

    // After replication: creating another thread on remote should continue counter (-> 2)
    let new_child = create_group_thread(&remote, &group_id, None);
    assert_thread_suffix(&new_child.thread_id, 2);

    // And sending another message on remote should continue message counter (-> 1)
    let new_msg = send_group_message(&remote, &group_id, None, "from remote");
    assert_message_suffix(&new_msg.message.message_id, 1);

    print_to_terminal(0, "crdt_core: replication/allocator test done");
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

#[derive(Deserialize)]
struct GetGroupRes {
    group: Option<serde_json::Value>,
}

/// Verifies the hypothesis: an incremental update (using remote SV) may not seed
/// the remote with freshly created groups, whereas a full update (SV = None) does.
/// This function logs the presence/absence of the group after each path and only
/// fails if the full update does not populate the group.
pub fn verify_incremental_vs_full_replication(local_node: &str, remote_node: &str) {
    print_to_terminal(0, "crdt_core: verify incremental vs full replication start");
    let local = chat_process_address(local_node);
    let remote = chat_process_address(remote_node);

    // 1) Create a new group on local
    let create_group_res = create_group(&local, "CRDT Verify Group");
    let group_id = create_group_res.group_id.clone();

    // 2) Attempt to seed remote via INCREMENTAL update
    let remote_sv = get_crdt_state_vector(&remote);
    let inc = crdt_update(&local, Some(remote_sv.state_vector))
        .unwrap_or_else(|e| fail_with(format!("incremental update failed: {e}")));
    if inc.update_payload.is_empty() {
        print_to_terminal(0, "crdt_core: WARN incremental update returned empty payload");
    }
    let _ = crdt_apply_update(&remote, &inc.update_payload)
        .unwrap_or_else(|e| fail_with(format!("apply incremental failed: {e}")));
    let exists_after_inc = get_group_exists(&remote, &group_id);
    print_to_terminal(0, &format!(
        "crdt_core: group present after incremental? {}",
        exists_after_inc
    ));

    // 3) Seed remote via FULL update
    let full = crdt_update(&local, None)
        .unwrap_or_else(|e| fail_with(format!("full update failed: {e}")));
    if full.update_payload.is_empty() {
        print_to_terminal(0, "crdt_core: WARN full update returned empty payload");
    }
    let _ = crdt_apply_update(&remote, &full.update_payload)
        .unwrap_or_else(|e| fail_with(format!("apply full failed: {e}")));
    let exists_after_full = get_group_exists(&remote, &group_id);
    print_to_terminal(0, &format!(
        "crdt_core: group present after full? {}",
        exists_after_full
    ));

    if !exists_after_full {
        fail_with("expected group to be present on remote after full update");
    }

    print_to_terminal(0, "crdt_core: verify incremental vs full replication done");
}

fn get_group_exists(address: &Address, group_id: &str) -> bool {
    let payload = json!({
        "GetGroup": { "group_id": group_id }
    });
    let result: Result<GetGroupRes, String> = send_chat_rpc(address, payload);
    match result {
        Ok(res) => res.group.is_some(),
        Err(err) => {
            print_to_terminal(0, &format!(
                "crdt_core: get_group_exists RPC error: {}",
                err
            ));
            false
        }
    }
}

fn get_crdt_state_vector(address: &Address) -> CrdtStateVectorRes {
    let payload = json!({ "CrdtStateVector": null });
    let result: Result<CrdtStateVectorRes, String> = send_chat_rpc(address, payload);
    unwrap_chat_result(result)
}

fn crdt_update(address: &Address, state_vector: Option<String>) -> Result<CrdtUpdateRes, String> {
    let sv_value = match state_vector {
        Some(s) => Value::String(s),
        None => Value::Null,
    };
    let payload = json!({
        "CrdtUpdate": {
            "state_vector": sv_value
        }
    });
    send_chat_rpc(address, payload)
}

fn crdt_apply_update(address: &Address, update_payload: &str) -> Result<CrdtApplyRes, String> {
    let payload = json!({
        "CrdtApplyUpdate": { "update_payload": update_payload }
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
