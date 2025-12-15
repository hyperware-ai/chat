use hyperware_process_lib::{print_to_terminal, Address, ProcessId, Request};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

use crate::fail;

pub fn run_replication_admin_tests(local_node: &str, remote_node: &str) {
    print_to_terminal(0, "replication_admin: start");
    let local = chat_process_address(local_node);
    let remote = chat_process_address(remote_node);

    // Create group and sync to remote.
    let group_id = create_group(&local, "Replication Admin Group").group_id;
    let snapshot = crdt_group_snapshot(&local, &group_id)
        .unwrap_or_else(|e| fail_with(format!("snapshot fetch failed: {e}")));
    crdt_group_apply_update(&remote, &group_id, &snapshot.update_payload)
        .unwrap_or_else(|e| fail_with(format!("snapshot apply failed: {e}")));

    // Run replication_work on both nodes; should return unit.
    let _: () = replication_work(&local).unwrap_or_else(|e| fail_with(format!("local repl failed: {e}")));
    let _: () = replication_work(&remote).unwrap_or_else(|e| fail_with(format!("remote repl failed: {e}")));

    // Admin replication state should include the group.
    let admin_state = admin_replication_state(&local, Some(&group_id))
        .unwrap_or_else(|e| fail_with(format!("admin_replication_state failed: {e}")));
    if admin_state
        .groups
        .iter()
        .all(|g| g.group_id != group_id)
    {
        fail_with("group missing from admin replication state");
    }

    // Admin whitelist should include the owner node with publish access.
    let whitelist = admin_whitelist(&local, &group_id)
        .unwrap_or_else(|e| fail_with(format!("admin_whitelist failed: {e}")));
    if whitelist
        .entries
        .iter()
        .all(|e| e.node != local_node)
    {
        fail_with("owner missing from whitelist");
    }

    // Admin subscriber events should be readable and clearable.
    let events_before =
        admin_subscriber_events(&local, false, Some(10)).unwrap_or_else(|e| fail_with(format!(
            "admin_subscriber_events (before) failed: {e}"
        )));
    if !events_before.events.is_empty() && events_before.events.len() > 10 {
        fail_with("subscriber events exceeded requested take");
    }
    let events_after =
        admin_subscriber_events(&local, true, Some(10)).unwrap_or_else(|e| fail_with(format!(
            "admin_subscriber_events (clear) failed: {e}"
        )));
    if !events_after.events.is_empty() {
        fail_with("subscriber events not cleared");
    }

    print_to_terminal(0, "replication_admin: done");
}

// ---------- RPC helpers ----------

#[derive(Deserialize)]
struct CreateGroupRes {
    group_id: String,
}

#[derive(Deserialize)]
struct CrdtUpdateRes {
    update_payload: String,
}

#[derive(Deserialize)]
struct CrdtApplyRes {
    applied: bool,
}

#[derive(Deserialize)]
struct AdminReplicationStateRes {
    metrics: Value,
    groups: Vec<GroupReplicationState>,
    #[serde(flatten)]
    _extra: HashMap<String, Value>,
}

#[derive(Deserialize)]
struct GroupReplicationState {
    group_id: String,
    pending_bootstrap: bool,
    hubs: Vec<String>,
    subscribers: Vec<String>,
    #[serde(flatten)]
    _extra: HashMap<String, Value>,
}

#[derive(Deserialize)]
struct AdminWhitelistRes {
    group_id: String,
    version: u64,
    entries: Vec<WhitelistEntryDebug>,
}

#[derive(Deserialize)]
struct WhitelistEntryDebug {
    node: String,
    publish: Vec<String>,
    subscribe: Vec<String>,
    audiences: Vec<String>,
    features: Vec<String>,
    #[serde(default)]
    expires_at: Option<u64>,
}

#[derive(Deserialize)]
struct SubscriberEventsRes {
    events: Vec<Value>,
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

fn crdt_group_snapshot(address: &Address, group_id: &str) -> Result<CrdtUpdateRes, String> {
    let payload = json!({
        "CrdtGroupSnapshot": { "group_id": group_id }
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

fn replication_work(address: &Address) -> Result<(), String> {
    let payload = json!({ "ReplicationWork": Value::Null });
    send_chat_rpc(address, payload)
}

fn admin_replication_state(
    address: &Address,
    group_id: Option<&str>,
) -> Result<AdminReplicationStateRes, String> {
    let payload = json!({
        "AdminReplicationState": {
            "group_id": group_id.map(Value::from).unwrap_or(Value::Null)
        }
    });
    send_chat_rpc(address, payload)
}

fn admin_whitelist(address: &Address, group_id: &str) -> Result<AdminWhitelistRes, String> {
    let payload = json!({
        "AdminWhitelist": { "group_id": group_id }
    });
    send_chat_rpc(address, payload)
}

fn admin_subscriber_events(
    address: &Address,
    clear: bool,
    take: Option<usize>,
) -> Result<SubscriberEventsRes, String> {
    let payload = json!({
        "AdminSubscriberEvents": {
            "clear": clear,
            "take": take
                .map(|t| Value::from(t as u64))
                .unwrap_or(Value::Null)
        }
    });
    send_chat_rpc(address, payload)
}

fn send_chat_rpc<T: DeserializeOwned>(address: &Address, payload: Value) -> Result<T, String> {
    let payload_dbg = payload.clone();
    print_to_terminal(
        0,
        format!(
            "replication_admin: rpc -> {}@{} payload {}",
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
        .unwrap_or_else(|_| {
            fail_with(format!(
                "chat request returned no response (payload={payload_dbg})"
            ))
        });

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
    let log = format!("replication_admin: error: {message}");
    print_to_terminal(0, log.as_str());
    fail!(message);
}

fn chat_process_address(node: &str) -> Address {
    Address {
        node: node.to_string(),
        process: ProcessId::new(Some("chat"), "chat", "ware.hypr"),
    }
}
