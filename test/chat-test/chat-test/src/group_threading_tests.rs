use hyperware_process_lib::{print_to_terminal, Address, ProcessId, Request};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

use crate::fail;

pub fn run_group_threading_tests(local_node: &str) {
    print_to_terminal(0, "group_threading: start");
    let local = chat_process_address(local_node);

    let create_group_res = create_group(&local, "Threading Test Group");
    let group_id = create_group_res.group_id;

    // Fetch root thread id and initial updated_at.
    let group = get_group(&local, &group_id)
        .unwrap_or_else(|e| fail_with(format!("get_group failed: {e}")))
        .group
        .unwrap_or_else(|| fail_with("group missing after create"));
    let root_thread_id = group
        .metadata
        .as_ref()
        .map(|m| m.root_thread_id.clone())
        .unwrap_or_else(|| fail_with("root thread id missing"));
    let orig_updated_at = group
        .metadata
        .as_ref()
        .map(|m| m.updated_at)
        .unwrap_or(0);

    // Create a child under root.
    let child = create_group_thread(&local, &group_id, None);
    assert_thread_parent(&local, &group_id, &child.thread_id, Some(&root_thread_id), 0);

    // Create a nested child under the first child.
    let grandchild = create_group_thread(&local, &group_id, Some(&child.thread_id));
    assert_thread_parent(&local, &group_id, &grandchild.thread_id, Some(&child.thread_id), 1);

    // Ensure parent recorded the child thread.
    let parent = get_group(&local, &group_id)
        .unwrap()
        .group
        .unwrap()
        .threads
        .get(&child.thread_id)
        .cloned()
        .unwrap_or_else(|| fail_with("parent thread missing"));
    if !parent.child_threads.contains(&grandchild.thread_id) {
        fail_with("parent thread missing grandchild reference");
    }

    // Metadata.updated_at should have advanced.
    let updated_group = get_group(&local, &group_id)
        .unwrap()
        .group
        .unwrap();
    let new_updated_at = updated_group
        .metadata
        .as_ref()
        .map(|m| m.updated_at)
        .unwrap_or(0);
    if new_updated_at <= orig_updated_at {
        fail_with("metadata.updated_at did not advance after thread creation");
    }

    print_to_terminal(0, "group_threading: done");
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
struct GetGroupRes {
    group: Option<GroupLite>,
}

#[derive(Deserialize, Clone)]
struct GroupLite {
    metadata: Option<GroupMetadataLite>,
    threads: HashMap<String, ThreadLite>,
}

#[derive(Deserialize, Clone)]
struct GroupMetadataLite {
    name: String,
    description: Option<String>,
    avatar: Option<String>,
    creator_id: String,
    created_at: u64,
    updated_at: u64,
    visibility: String,
    default_role_id: String,
    root_thread_id: String,
}

#[derive(Deserialize, Clone)]
struct ThreadLite {
    id: String,
    group_id: String,
    depth: u32,
    parent: ThreadParentRefLite,
    child_threads: Vec<String>,
}

#[derive(Deserialize, Clone, Debug)]
enum ThreadParentRefLite {
    Root(String),
    Thread(String),
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

fn get_group(address: &Address, group_id: &str) -> Result<GetGroupRes, String> {
    let payload = json!({
        "GetGroup": { "group_id": group_id }
    });
    send_chat_rpc(address, payload)
}

fn assert_thread_parent(
    address: &Address,
    group_id: &str,
    thread_id: &str,
    expected_parent: Option<&str>,
    expected_depth: u32,
) {
    let group = get_group(address, group_id)
        .unwrap()
        .group
        .unwrap_or_else(|| fail_with("group missing while asserting thread parent"));
    let thread = group
        .threads
        .get(thread_id)
        .cloned()
        .unwrap_or_else(|| fail_with(format!("thread {thread_id} missing")));
    if thread.depth != expected_depth {
        fail_with(format!(
            "expected thread depth {} for {}, got {}",
            expected_depth, thread_id, thread.depth
        ));
    }
    match (&thread.parent, expected_parent) {
        (ThreadParentRefLite::Root(_), None) => {}
        (ThreadParentRefLite::Root(root), Some(exp)) if root == exp => {}
        (ThreadParentRefLite::Thread(parent), Some(exp)) if parent == exp => {}
        other => fail_with(format!("unexpected parent for {thread_id}: {:?}", other)),
    }
}

fn send_chat_rpc<T: DeserializeOwned>(address: &Address, payload: Value) -> Result<T, String> {
    print_to_terminal(
        0,
        format!(
            "group_threading: rpc -> {}@{} payload {}",
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
    let log = format!("group_threading: error: {message}");
    print_to_terminal(0, log.as_str());
    fail!(message);
}

fn chat_process_address(node: &str) -> Address {
    Address {
        node: node.to_string(),
        process: ProcessId::new(Some("chat"), "chat", "ware.hypr"),
    }
}
