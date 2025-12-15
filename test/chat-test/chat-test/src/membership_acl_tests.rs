use hyperware_process_lib::{print_to_terminal, Address, ProcessId, Request};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::{thread, time::Duration};

use crate::fail;

pub fn run_membership_acl_tests(local_node: &str, remote_node: &str) {
    print_to_terminal(0, "membership_acl: start");
    let local = chat_process_address(local_node);
    let remote = chat_process_address(remote_node);

    // Create a fresh group and bootstrap the remote with a snapshot.
    let create_group_res = create_group(&local, "ACL Membership Group");
    let group_id = create_group_res.group_id;

    let snapshot = crdt_group_snapshot(&local, &group_id)
        .unwrap_or_else(|e| fail_with(format!("snapshot fetch failed: {e}")));
    crdt_group_apply_update(&remote, &group_id, &snapshot.update_payload)
        .unwrap_or_else(|e| fail_with(format!("snapshot apply on remote failed: {e}")));

    // Remote should not be able to send a message yet.
    let denied = send_group_message_expect_error(&remote, &group_id, None, "should fail");
    if !denied.contains("cannot send group message") && !denied.contains("permission") {
        fail_with(format!(
            "expected permission error for non-member, got: {denied}"
        ));
    }

    // Invite the remote node as a subscriber-tier member.
    let default_role = format!("{group_id}:member");
    let invite = invite_group_member(&local, &group_id, remote_node, &default_role);
    match invite.decision.status.as_str() {
        "Approved" => {}
        "Pending" => {
            let proposal_id = membership_proposal_id(&group_id, remote_node, "invite");
            let approval = approve_group_membership(&local, &group_id, &proposal_id);
            if approval.decision.status != "Approved" {
                fail_with("expected membership approval to succeed");
            }
        }
        other => fail_with(format!("unexpected membership decision: {other}")),
    }

    // Sync the updated CRDT to the remote.
    let update = crdt_group_update(&local, &group_id, None)
        .unwrap_or_else(|e| fail_with(format!("crdt_group_update after invite failed: {e}")));
    crdt_group_apply_update(&remote, &group_id, &update.update_payload)
        .unwrap_or_else(|e| fail_with(format!("apply invite update on remote failed: {e}")));

    // Nudge replication housekeeping so pending_bootstrap can be cleared when ACL is ready.
    replication_work(&remote).unwrap_or_else(|e| fail_with(format!("remote replication_work failed: {e}")));

    // Wait until the remote marks the group as bootstrapped and reflects the new member
    wait_until_bootstrapped(&remote, &group_id);
    wait_for_remote_member(&remote, &group_id, remote_node);

    // Validate membership + subscriber sets on remote.
    let remote_group = get_group(&remote, &group_id)
        .unwrap_or_else(|e| fail_with(format!("get_group on remote failed: {e}")));
    let group = remote_group.group.unwrap_or_else(|| fail_with("remote group missing"));
    assert_member_status(&group.members, remote_node, "Active");
    assert!(
        group.subscribers.entries.contains_key(remote_node),
        "remote should be in subscriber set"
    );
    assert!(
        !group.hubs.active.iter().any(|n| n == remote_node),
        "remote should not be a hub for default member role"
    );

    // Remote can now send.
    let msg = send_group_message(&remote, &group_id, None, "hello as member")
        .unwrap_or_else(|e| fail_with(format!("remote send failed: {e}")));
    if msg.message.message_id.is_empty() {
        fail_with("expected message id in remote send response");
    }

    // Remove the remote member.
    let removal = remove_group_member(&local, &group_id, remote_node);
    if removal.decision.status != "Approved" {
        fail_with("expected removal to be approved");
    }
    let removal_update = crdt_group_update(&local, &group_id, None)
        .unwrap_or_else(|e| fail_with(format!("crdt_group_update after removal failed: {e}")));
    crdt_group_apply_update(&remote, &group_id, &removal_update.update_payload)
        .unwrap_or_else(|e| fail_with(format!("apply removal update on remote failed: {e}")));

    let post_group = get_group(&remote, &group_id)
        .unwrap_or_else(|e| fail_with(format!("get_group after removal failed: {e}")))
        .group
        .unwrap_or_else(|| fail_with("remote group missing after removal"));
    assert_member_status(&post_group.members, remote_node, "Removed");
    assert!(
        !post_group.subscribers.entries.contains_key(remote_node),
        "remote should be removed from subscriber set"
    );
    if post_group.hubs.active.iter().any(|n| n == remote_node) {
        fail_with("remote should not be hub after removal");
    }

    // Confirm send now fails again.
    let denied_after = send_group_message_expect_error(&remote, &group_id, None, "fail again");
    if !denied_after.contains("cannot send group message") && !denied_after.contains("permission")
    {
        fail_with(format!(
            "expected permission error after removal, got: {denied_after}"
        ));
    }

    // Check whitelist does not include remote anymore.
    let whitelist = admin_whitelist(&local, &group_id)
        .unwrap_or_else(|e| fail_with(format!("admin_whitelist failed: {e}")));
    if whitelist
        .entries
        .iter()
        .any(|e| e.node == remote_node)
    {
        fail_with("remote node still present in whitelist after removal");
    }

    // Replication admin snapshot should report group and not pending bootstrap.
    let admin_state = admin_replication_state(&local, Some(&group_id))
        .unwrap_or_else(|e| fail_with(format!("admin_replication_state failed: {e}")));
    if let Some(state) = admin_state
        .groups
        .iter()
        .find(|g| g.group_id == group_id)
    {
        if state.pending_bootstrap {
            fail_with("group unexpectedly pending bootstrap in admin state");
        }
    } else {
        fail_with("group missing in admin replication state");
    }

    print_to_terminal(0, "membership_acl: done");
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
struct GetGroupRes {
    group: Option<GroupLite>,
}

#[derive(Deserialize)]
struct GroupLite {
    members: HashMap<String, GroupMemberLite>,
    hubs: HubSetLite,
    subscribers: SubscriberSetLite,
    #[serde(flatten)]
    _extra: HashMap<String, Value>,
}

#[derive(Deserialize)]
struct GroupMemberLite {
    role_id: String,
    status: String,
    last_activity: u64,
    #[serde(flatten)]
    _extra: HashMap<String, Value>,
}

#[derive(Deserialize)]
struct HubSetLite {
    active: Vec<String>,
    #[serde(flatten)]
    _extra: HashMap<String, Value>,
}

#[derive(Deserialize)]
struct SubscriberSetLite {
    entries: HashMap<String, Value>,
    #[serde(flatten)]
    _extra: HashMap<String, Value>,
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

fn crdt_group_update(
    address: &Address,
    group_id: &str,
    state_vector: Option<String>,
) -> Result<CrdtUpdateRes, String> {
    let payload = json!({
        "CrdtGroupUpdate": {
            "group_id": group_id,
            "state_vector": state_vector.map(Value::from).unwrap_or(Value::Null)
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
            "content": content,
            "message_type": "Text",
            "reply_to": Value::Null,
            "attachments": []
        }
    });
    send_chat_rpc(address, payload)
}

fn send_group_message_expect_error(
    address: &Address,
    group_id: &str,
    thread_id: Option<&str>,
    content: &str,
) -> String {
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
    let result: Result<Value, String> = send_chat_rpc(address, payload);
    match result {
        Ok(Value::String(s)) => s,
        Ok(other) => format!("unexpected success value: {other}"),
        Err(err) => err,
    }
}

fn invite_group_member(
    address: &Address,
    group_id: &str,
    candidate: &str,
    role_id: &str,
) -> MembershipDecisionRes {
    let payload = json!({
        "InviteGroupMember": {
            "group_id": group_id,
            "candidate": candidate,
            "role_id": role_id
        }
    });
    let result: Result<MembershipDecisionRes, String> = send_chat_rpc(address, payload);
    unwrap_chat_result(result)
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

fn remove_group_member(
    address: &Address,
    group_id: &str,
    member: &str,
) -> MembershipDecisionRes {
    let payload = json!({
        "RemoveGroupMember": {
            "group_id": group_id,
            "member": member
        }
    });
    let result: Result<MembershipDecisionRes, String> = send_chat_rpc(address, payload);
    unwrap_chat_result(result)
}

fn get_group(address: &Address, group_id: &str) -> Result<GetGroupRes, String> {
    let payload = json!({
        "GetGroup": { "group_id": group_id }
    });
    send_chat_rpc(address, payload)
}

fn admin_whitelist(address: &Address, group_id: &str) -> Result<AdminWhitelistRes, String> {
    let payload = json!({
        "AdminWhitelist": { "group_id": group_id }
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

fn wait_until_bootstrapped(address: &Address, group_id: &str) {
    for _ in 0..30 {
        match admin_replication_state(address, Some(group_id)) {
            Ok(state) => {
                if state
                    .groups
                    .iter()
                    .any(|g| g.group_id == group_id && !g.pending_bootstrap)
                {
                    return;
                }
            }
            Err(_) => {}
        }
        thread::sleep(Duration::from_millis(100));
    }
    fail_with("group still pending bootstrap");
}

fn wait_for_remote_member(address: &Address, group_id: &str, node: &str) {
    for _ in 0..30 {
        match get_group(address, group_id) {
            Ok(res) => {
                if let Some(g) = res.group {
                    if g.members.contains_key(node) {
                        return;
                    }
                }
            }
            Err(_) => {}
        }
        thread::sleep(Duration::from_millis(100));
    }
    fail_with(format!("member {node} missing after apply"));
}

fn send_chat_rpc<T: DeserializeOwned>(address: &Address, payload: Value) -> Result<T, String> {
    print_to_terminal(
        0,
        format!(
            "membership_acl: rpc -> {}@{} payload {}",
            address.process, address.node, payload
        )
        .as_str(),
    );
    let body = serde_json::to_vec(&payload)
        .unwrap_or_else(|e| fail_with(format!("failed to encode chat payload: {e}")));

    let response = Request::to(address.clone())
        .body(body)
        .send_and_await_response(30)
        .map_err(|e| format!("failed to send chat request: {e:?}"))?
        .map_err(|e| format!("chat request returned send error: {e:?}"))?;

    if response.is_request() {
        fail_with("chat request returned a request instead of a response");
    }

    let rpc_result: Result<T, String> = serde_json::from_slice(response.body())
        .map_err(|e| format!("failed to decode chat response: {e}"))?;

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
    let log = format!("membership_acl: error: {message}");
    print_to_terminal(0, log.as_str());
    fail!(message);
}

fn chat_process_address(node: &str) -> Address {
    Address {
        node: node.to_string(),
        process: ProcessId::new(Some("chat"), "chat", "ware.hypr"),
    }
}

fn membership_proposal_id(group_id: &str, candidate: &str, action: &str) -> String {
    format!("{group_id}:{action}:{candidate}")
}

fn assert_member_status(
    members: &HashMap<String, GroupMemberLite>,
    node: &str,
    expected: &str,
) {
    let status = members
        .get(node)
        .unwrap_or_else(|| fail_with(format!("member {node} missing")))
        .status
        .clone();
    if status != expected {
        fail_with(format!(
            "expected member {node} status {expected}, got {status}"
        ));
    }
}
