use hyperware_process_lib::{print_to_terminal, Address, ProcessId, Request};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::{thread, time::Duration};

use crate::fail;

pub fn run_multi_replication_tests(node_names: &[String]) {
    if node_names.len() < 4 {
        fail!("multi_replication_tests requires at least 4 nodes");
    }
    let hub_a = &node_names[0];
    let hub_b = &node_names[1];
    let sub_a = &node_names[2];
    let sub_b = &node_names[3];

    print_to_terminal(
        0,
        format!(
            "multi_repl: using hubs=({}, {}) subs=({}, {})",
            hub_a, hub_b, sub_a, sub_b
        )
        .as_str(),
    );

    let addr_hub_a = chat_process_address(hub_a);
    let addr_hub_b = chat_process_address(hub_b);
    let addr_sub_a = chat_process_address(sub_a);
    let addr_sub_b = chat_process_address(sub_b);

    // Create group and enroll peers with roles
    let create = create_group(&addr_hub_a, "Multi Hub/Sub Group");
    let group_id = create.group_id;
    let hub_role = format!("{group_id}:owner");
    let member_role = format!("{group_id}:member");

    invite_and_activate(&addr_hub_a, &group_id, hub_b, &hub_role);
    invite_and_activate(&addr_hub_a, &group_id, sub_a, &member_role);
    invite_and_activate(&addr_hub_a, &group_id, sub_b, &member_role);

    // Fan out snapshot from hub_a so everyone starts from the same state.
    let snapshot = crdt_group_snapshot(&addr_hub_a, &group_id)
        .unwrap_or_else(|e| fail_with(format!("snapshot fetch failed: {e}")));
    for target in [&addr_hub_b, &addr_sub_a, &addr_sub_b] {
        crdt_group_apply_update(target, &group_id, &snapshot.update_payload)
            .unwrap_or_else(|e| fail_with(format!("apply snapshot on {} failed: {e}", target.node)));
        replication_work(target)
            .unwrap_or_else(|e| fail_with(format!("replication_work on {} failed: {e}", target.node)));
    }
    replication_work(&addr_hub_a)
        .unwrap_or_else(|e| fail_with(format!("replication_work on hub_a failed: {e}")));

    // Validate membership sets on all peers
    for addr in [&addr_hub_a, &addr_hub_b, &addr_sub_a, &addr_sub_b] {
        let group = get_group(addr, &group_id)
            .unwrap_or_else(|e| fail_with(format!("get_group on {} failed: {e}", addr.node)))
            .group
            .unwrap_or_else(|| fail_with(format!("group missing on {}", addr.node)));
        assert!(group.members.contains_key(hub_a));
        assert!(group.members.contains_key(hub_b));
        assert!(group.members.contains_key(sub_a));
        assert!(group.members.contains_key(sub_b));
        // Hubs set should include both hub nodes; subscribers set should include subs
        assert!(
            group.hubs.active.iter().any(|n| n == hub_a) && group.hubs.active.iter().any(|n| n == hub_b),
            "hubs set on {} missing hub members",
            addr.node
        );
        assert!(
            group.subscribers.entries.contains_key(sub_a) && group.subscribers.entries.contains_key(sub_b),
            "subscriber set on {} missing entries",
            addr.node
        );
    }

    // Send from hub_a -> everyone should see it
    let msg_a = send_group_message(&addr_hub_a, &group_id, None, "from hub_a")
        .unwrap_or_else(|e| fail_with(format!("hub_a send failed: {e}")))
        .message
        .message_id;
    replication_work(&addr_hub_a)
        .unwrap_or_else(|e| fail_with(format!("replication_work after hub_a send failed: {e}")));
    wait_for_group_message(&addr_hub_b, &group_id, &msg_a);
    wait_for_group_message(&addr_sub_a, &group_id, &msg_a);
    wait_for_group_message(&addr_sub_b, &group_id, &msg_a);

    // Send from hub_b -> everyone should see it
    let msg_b = send_group_message(&addr_hub_b, &group_id, None, "from hub_b")
        .unwrap_or_else(|e| fail_with(format!("hub_b send failed: {e}")))
        .message
        .message_id;
    replication_work(&addr_hub_b)
        .unwrap_or_else(|e| fail_with(format!("replication_work after hub_b send failed: {e}")));
    wait_for_group_message(&addr_hub_a, &group_id, &msg_b);
    wait_for_group_message(&addr_sub_a, &group_id, &msg_b);
    wait_for_group_message(&addr_sub_b, &group_id, &msg_b);

    // Send from subscriber A -> should reach hubs and other subscriber
    let msg_sub = send_group_message(&addr_sub_a, &group_id, None, "from sub_a")
        .unwrap_or_else(|e| fail_with(format!("sub_a send failed: {e}")))
        .message
        .message_id;
    replication_work(&addr_sub_a)
        .unwrap_or_else(|e| fail_with(format!("replication_work after sub_a send failed: {e}")));
    wait_for_group_message(&addr_hub_a, &group_id, &msg_sub);
    wait_for_group_message(&addr_hub_b, &group_id, &msg_sub);
    wait_for_group_message(&addr_sub_b, &group_id, &msg_sub);

    // Check replication cursors on hub_a to ensure both lanes moved.
    let admin = admin_replication_state(&addr_hub_a, Some(&group_id))
        .unwrap_or_else(|e| fail_with(format!("admin_replication_state failed: {e}")));
    let state = admin
        .groups
        .into_iter()
        .find(|g| g.group_id == group_id)
        .unwrap_or_else(|| fail_with("group missing in admin_replication_state"));
    assert!(
        state.hub_cursors.contains_key(hub_b),
        "hub_a missing hub cursor for hub_b"
    );
    assert!(
        state.subscriber_cursors.contains_key(sub_a)
            && state.subscriber_cursors.contains_key(sub_b),
        "hub_a missing subscriber cursors for subs"
    );

    print_to_terminal(0, "multi_repl: completed");
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
struct MembershipDecisionRes {
    decision: MembershipDecisionLite,
}

#[derive(Deserialize)]
struct MembershipDecisionLite {
    status: String,
    #[serde(default)]
    missing_signatures: Vec<String>,
    #[serde(default)]
    reason: Option<String>,
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
    group: Option<GroupLite>,
}

#[derive(Deserialize)]
struct GroupLite {
    #[serde(default)]
    members: HashMap<String, MemberLite>,
    #[serde(default)]
    messages: HashMap<String, MessageMetaLite>,
    #[serde(default)]
    hubs: HubsLite,
    #[serde(default)]
    subscribers: SubscribersLite,
}

#[derive(Deserialize)]
struct MemberLite {
    #[serde(default)]
    status: String,
}

#[derive(Deserialize, Default)]
struct HubsLite {
    #[serde(default)]
    active: Vec<String>,
}

#[derive(Deserialize, Default)]
struct SubscribersLite {
    #[serde(default)]
    entries: HashMap<String, SubscriberEntryLite>,
}

#[derive(Deserialize, Default)]
struct SubscriberEntryLite {
    #[serde(default)]
    last_seen_ts: u64,
}

#[derive(Deserialize)]
struct AdminReplicationStateRes {
    groups: Vec<GroupReplicationState>,
    #[allow(dead_code)]
    metrics: AdminMetrics,
}

#[derive(Deserialize)]
struct GroupReplicationState {
    group_id: String,
    pending_bootstrap: bool,
    hubs: Vec<String>,
    subscribers: Vec<String>,
    hub_cursors: HashMap<String, DeliveryCursorLite>,
    subscriber_cursors: HashMap<String, DeliveryCursorLite>,
}

#[derive(Deserialize)]
struct DeliveryCursorLite {
    queue_id: String,
    last_offset: u64,
    #[serde(default)]
    updated_at: u64,
}

#[derive(Deserialize)]
struct AdminMetrics {
    #[serde(default)]
    last_lag_secs: u64,
    #[serde(default)]
    last_subscriber_lag_secs: u64,
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
            "multi_repl: rpc -> {}@{} payload {}",
            address.process, address.node, payload
        )
        .as_str(),
    );
    let body = serde_json::to_vec(&payload).map_err(|e| format!("{e:?}"))?;

    let response = Request::to(address.clone())
        .body(body)
        .send_and_await_response(30)
        .map_err(|e| format!("failed to send chat request: {e:?}"))?
        .map_err(|e| format!("chat request returned send error: {e:?}"))?;

    if response.is_request() {
        fail_with("chat request returned a request instead of a response");
    }

    let rpc_result: Result<T, String> =
        serde_json::from_slice(response.body()).map_err(|e| format!("decode error: {e}"))?;

    rpc_result
}

fn create_group(address: &Address, name: &str) -> CreateGroupRes {
    let payload = json!({ "CreateGroup": {
        "name": name,
        "description": Value::Null,
        "avatar": Value::Null,
        "visibility": Value::Null,
        "default_role_label": Value::Null,
        "root_thread_title": Value::Null,
        "group_id": Value::Null,
        "membership_rules": []
    } });
    let result: Result<CreateGroupRes, String> = send_chat_rpc(address, payload);
    unwrap_chat_result(result)
}

fn invite_and_activate(address: &Address, group_id: &str, candidate: &str, role_id: &str) {
    let payload = json!({
        "InviteGroupMember": {
            "group_id": group_id,
            "candidate": candidate,
            "role_id": role_id
        }
    });
    let result: Result<MembershipDecisionRes, String> = send_chat_rpc(address, payload);
    let decision = unwrap_chat_result(result).decision;
    if decision.status == "Pending" {
        let proposal_id = membership_proposal_id(group_id, candidate, "invite");
        let approval = approve_group_membership(address, group_id, &proposal_id);
        if approval.decision.status != "Approved" {
            fail_with(format!(
                "expected invite approval for {candidate}, got {:?}",
                approval.decision.status
            ));
        }
    } else if decision.status != "Approved" {
        fail_with(format!(
            "unexpected invite decision for {candidate}: {}",
            decision.status
        ));
    }
}

fn membership_proposal_id(group_id: &str, node: &str, action: &str) -> String {
    format!("{group_id}:{action}:{node}")
}

fn approve_group_membership(
    address: &Address,
    group_id: &str,
    proposal_id: &str,
) -> MembershipDecisionRes {
    let payload = json!({
        "ApproveGroupMembership": {
            "group_id": group_id,
            "proposal_id": proposal_id
        }
    });
    let result: Result<MembershipDecisionRes, String> = send_chat_rpc(address, payload);
    unwrap_chat_result(result)
}

fn crdt_group_snapshot(address: &Address, group_id: &str) -> Result<CrdtUpdateRes, String> {
    let payload = json!({
        "CrdtGroupSnapshot": {
            "group_id": group_id
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
            "update_payload": update_payload,
            "acl_version": Value::Null
        }
    });
    send_chat_rpc(address, payload)
}

fn send_group_message(
    address: &Address,
    group_id: &str,
    thread_id: Option<&str>,
    content: &str,
) -> Result<SendGroupMessageRes, String> {
    let thread = thread_id.map(Value::from).unwrap_or(Value::Null);
    let payload = json!({
        "SendGroupMessage": {
            "group_id": group_id,
            "thread_id": thread,
            "reply_to": Value::Null,
            "message_type": "Text",
            "attachments": [],
            "content": content
        }
    });
    send_chat_rpc(address, payload)
}

fn get_group(address: &Address, group_id: &str) -> Result<GetGroupRes, String> {
    let payload = json!({
        "GetGroup": { "group_id": group_id }
    });
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

fn replication_work(address: &Address) -> Result<(), String> {
    let payload = json!({ "ReplicationWork": Value::Null });
    send_chat_rpc(address, payload)
}

fn wait_for_group_message(address: &Address, group_id: &str, message_id: &str) {
    for _ in 0..60 {
        if let Ok(res) = get_group(address, group_id) {
            if let Some(group) = res.group {
                if group.messages.values().any(|m| m.message_id == message_id)
                    || group.messages.keys().any(|k| k == message_id)
                {
                    return;
                }
            }
        }
        thread::sleep(Duration::from_millis(250));
    }
    fail_with(format!(
        "node {} did not observe group message {}",
        address.node, message_id
    ));
}

fn unwrap_chat_result<T>(result: Result<T, String>) -> T {
    match result {
        Ok(val) => val,
        Err(err) => fail_with(err),
    }
}

fn fail_with(message: impl Into<String>) -> ! {
    let msg = message.into();
    print_to_terminal(0, msg.as_str());
    fail!(msg);
}
