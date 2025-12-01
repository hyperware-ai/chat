use crate::crdt::{
    compile_membership_rules, Group, GroupCrdtManager, GroupDocState, GroupId, GroupMember,
    GroupPermissions, GroupRoutingConfig, MembershipActionKind, MembershipDecision,
    MembershipDecisionStatus, MembershipProposal, MembershipRuleBox, MembershipRuleConfig,
    MembershipRuleError, MembershipStatus, MessageId, MessageMeta, NodeId, SubscriberSyncState,
    ThreadId,
};
use crate::types::{
    active_member_count, aggregate_rule_decisions, current_timestamp, group_root_thread_id,
    membership_proposal_key, sync_member_membership_sets,
};
use crate::{log_crdt_event, ChatState};
use hyperware_crdt::yrs::Encode;
use hyperware_crdt::CommitteeError;
use hyperware_process_lib::our;
use std::collections::HashSet;

impl ChatState {
    pub fn rebuild_group_doc_managers(&mut self) -> Result<(), CommitteeError> {
        self.ensure_routing_defaults_for_all();
        self.group_doc_managers.clear();
        self.groups_pending_bootstrap.clear();
        for (group_id, group) in &self.groups {
            if self.should_seed_group_doc(group) {
                let manager = GroupCrdtManager::from_group(group_id, group)?;
                self.group_doc_managers.insert(group_id.clone(), manager);
            } else {
                self.groups_pending_bootstrap.insert(group_id.clone());
            }
        }
        self.pubsub.rebuild_all(&self.groups);
        Ok(())
    }

    pub fn rebuild_pubsub_for_group(&mut self, group_id: &GroupId) {
        if let Some(group) = self.groups.get(group_id) {
            self.pubsub.rebuild_group(group_id, group);
        } else {
            self.pubsub.remove_group(group_id);
        }
    }

    fn ensure_routing_defaults_for_all(&mut self) {
        for (group_id, group) in self.groups.iter_mut() {
            if group.routing.hub_topic.is_empty() || group.routing.subscriber_topic.is_empty() {
                group.routing = GroupRoutingConfig::for_group(group_id);
            }
        }
    }

    pub fn group_rules(
        &mut self,
        group_id: &GroupId,
    ) -> Result<&[MembershipRuleBox], MembershipRuleError> {
        if !self.membership_rule_cache.contains_key(group_id) {
            self.rebuild_group_rule_cache(group_id)?;
        }

        Ok(self
            .membership_rule_cache
            .get(group_id)
            .expect("group rules cache populated after rebuild")
            .as_slice())
    }

    pub fn invalidate_group_rules(&mut self, group_id: &GroupId) {
        self.membership_rule_cache.remove(group_id);
    }

    pub fn rebuild_group_rule_cache(
        &mut self,
        group_id: &GroupId,
    ) -> Result<(), MembershipRuleError> {
        let configs = self
            .groups
            .get(group_id)
            .map(|group| group.membership_rules.as_slice())
            .unwrap_or(&[]);
        let compiled = compile_membership_rules(configs)?;
        self.membership_rule_cache
            .insert(group_id.clone(), compiled);
        Ok(())
    }

    pub fn commit_group_crdt(&mut self, group_id: &GroupId) -> Result<(), CommitteeError> {
        if self.group_needs_bootstrap(group_id) {
            return Err(CommitteeError::Observer(format!(
                "group {} requires bootstrap before CRDT commit",
                group_id
            )));
        }

        let group = self.groups.get(group_id).ok_or_else(|| {
            CommitteeError::Observer(format!("missing group {} for CRDT commit", group_id))
        })?;
        self.pubsub.rebuild_group(group_id, group);
        let snapshot: GroupDocState = (group_id, group).into();
        let manager = self.ensure_group_doc_manager(group_id)?;
        manager.refresh_with_snapshot(snapshot)?;
        let state_vector = {
            let doc = manager.doc();
            let state_vector = doc.state_vector();
            log_crdt_event(doc.id(), "commit_group_crdt", &state_vector, None);
            state_vector
        };
        manager.set_last_state_vector(state_vector.clone());
        self.update_local_hub_sync_state(group_id, &state_vector);
        // enqueue per-peer fanout for hubs and subscribers
        self.enqueue_replication_pushes(group_id, state_vector.encode_v1());
        Ok(())
    }

    pub fn commit_group_crdt_or_log(&mut self, group_id: &GroupId, context: &str) {
        if let Err(err) = self.commit_group_crdt(group_id) {
            println!(
                "Failed to commit group CRDT state (group={} context={}): {:?}",
                group_id, context, err
            );
        }
    }

    pub fn set_group_membership_rules(
        &mut self,
        group_id: GroupId,
        rules: Vec<MembershipRuleConfig>,
    ) {
        let entry = self
            .groups
            .entry(group_id.clone())
            .or_insert_with(Group::default);
        entry.membership_rules = rules;
        self.invalidate_group_rules(&group_id);
        self.commit_group_crdt_or_log(&group_id, "set_group_membership_rules");
    }

    pub fn next_group_thread_id(&mut self, group_id: &GroupId) -> Result<ThreadId, String> {
        let group = self
            .groups
            .get_mut(group_id)
            .ok_or_else(|| "Group not found".to_string())?;
        Ok(group.counters.next_thread_id(group_id))
    }

    pub fn next_group_message_id(&mut self, group_id: &GroupId) -> Result<MessageId, String> {
        let group = self
            .groups
            .get_mut(group_id)
            .ok_or_else(|| "Group not found".to_string())?;
        Ok(group.counters.next_message_id(group_id))
    }

    pub fn send_group_message_state(
        &mut self,
        mut req: crate::SendGroupMessageReq,
    ) -> Result<crate::SendGroupMessageRes, String> {
        self.require_group_permission(&req.group_id, &our().node, GroupPermissions::SEND_MESSAGES)
            .map_err(|err| format!("cannot send group message: {}", err))?;
        self.require_subscriber_access(&req.group_id, &our().node)
            .map_err(|err| format!("cannot send group message: {}", err))?;

        let message_id = self.next_group_message_id(&req.group_id)?;
        let now = current_timestamp();
        let sender = our().node.clone();

        let message = {
            let group = self
                .groups
                .get_mut(&req.group_id)
                .ok_or_else(|| "Group not found".to_string())?;

            let thread_id = req
                .thread_id
                .take()
                .or_else(|| group_root_thread_id(group))
                .ok_or_else(|| "Group missing root thread".to_string())?;

            let thread = group
                .threads
                .get_mut(&thread_id)
                .ok_or_else(|| "Thread not found".to_string())?;

            let mut message = MessageMeta::new(
                message_id.clone(),
                thread_id.clone(),
                req.group_id.clone(),
                sender.clone(),
                now,
                req.message_type,
            );
            message.reply_to = req.reply_to.take();
            message.attachments = req.attachments.clone();

            if thread.root_message_id.is_none() {
                thread.root_message_id = Some(message_id.clone());
            }
            thread.summary.message_count += 1;
            thread.summary.last_message_id = Some(message_id.clone());
            thread.summary.last_activity = now;
            thread.summary.last_sender = Some(sender.clone());

            group.messages.insert(message_id.clone(), message.clone());
            if let Some(meta) = group.metadata.as_mut() {
                meta.updated_at = now;
            }

            let subscriber = group
                .subscribers
                .entries
                .entry(sender.clone())
                .or_insert_with(SubscriberSyncState::default);
            subscriber.last_seen_ts = now;

            message
        };
        self.commit_group_crdt_or_log(&req.group_id, "send_group_message");
        Ok(crate::SendGroupMessageRes { message })
    }

    pub fn invite_member(
        &mut self,
        group_id: &GroupId,
        proposer: NodeId,
        candidate: NodeId,
        role_id: String,
    ) -> Result<MembershipDecision, crate::MembershipActionError> {
        self.require_group_permission(group_id, &proposer, GroupPermissions::INVITE_MEMBERS)
            .map_err(crate::MembershipActionError::PermissionDenied)?;

        let proposal_id =
            membership_proposal_key(group_id, &candidate, MembershipActionKind::Invite);
        let eligible_voters = {
            let group = self
                .groups
                .get(group_id)
                .ok_or_else(|| crate::MembershipActionError::GroupNotFound(group_id.clone()))?;
            if let Some(member) = group.members.get(&candidate) {
                if member.status != MembershipStatus::Removed {
                    return Err(crate::MembershipActionError::MemberExists(candidate));
                }
            }
            if group.membership_proposals.contains_key(&proposal_id) {
                return Err(crate::MembershipActionError::ProposalExists(proposal_id));
            }
            active_member_count(group)
        };

        let mut proposal = MembershipProposal {
            proposal_id,
            candidate,
            requested_role: role_id,
            proposer,
            action: MembershipActionKind::Invite,
            approvals: HashSet::new(),
            rejections: HashSet::new(),
            eligible_voters,
            token_support: 0,
            token_opposition: 0,
        };
        proposal.approvals.insert(proposal.proposer.clone());
        self.process_membership_proposal(group_id, proposal)
    }

    pub fn approve_membership(
        &mut self,
        group_id: &GroupId,
        proposal_id: &str,
        approver: NodeId,
    ) -> Result<MembershipDecision, crate::MembershipActionError> {
        self.require_group_permission(group_id, &approver, GroupPermissions::INVITE_MEMBERS)
            .map_err(crate::MembershipActionError::PermissionDenied)?;

        let proposal = {
            let group = self
                .groups
                .get_mut(group_id)
                .ok_or_else(|| crate::MembershipActionError::GroupNotFound(group_id.clone()))?;
            let proposal = group
                .membership_proposals
                .get_mut(proposal_id)
                .ok_or_else(|| {
                    crate::MembershipActionError::ProposalNotFound(proposal_id.to_string())
                })?;
            proposal.approvals.insert(approver);
            proposal.clone()
        };
        self.process_membership_proposal(group_id, proposal)
    }

    pub fn remove_member(
        &mut self,
        group_id: &GroupId,
        proposer: NodeId,
        target: NodeId,
    ) -> Result<MembershipDecision, crate::MembershipActionError> {
        self.require_group_permission(group_id, &proposer, GroupPermissions::MANAGE_ROLES)
            .map_err(crate::MembershipActionError::PermissionDenied)?;

        let proposal_id = membership_proposal_key(group_id, &target, MembershipActionKind::Remove);
        let (eligible_voters, role_id) = {
            let group = self
                .groups
                .get(group_id)
                .ok_or_else(|| crate::MembershipActionError::GroupNotFound(group_id.clone()))?;
            let member = group
                .members
                .get(&target)
                .ok_or_else(|| crate::MembershipActionError::MemberNotFound(target.clone()))?;
            if member.status == MembershipStatus::Removed {
                return Err(crate::MembershipActionError::MemberNotFound(target));
            }
            if group.membership_proposals.contains_key(&proposal_id) {
                return Err(crate::MembershipActionError::ProposalExists(proposal_id));
            }
            (active_member_count(group), member.role_id.clone())
        };

        let mut proposal = MembershipProposal {
            proposal_id,
            candidate: target,
            requested_role: role_id,
            proposer,
            action: MembershipActionKind::Remove,
            approvals: HashSet::new(),
            rejections: HashSet::new(),
            eligible_voters,
            token_support: 0,
            token_opposition: 0,
        };
        proposal.approvals.insert(proposal.proposer.clone());
        self.process_membership_proposal(group_id, proposal)
    }

    fn evaluate_membership(
        &mut self,
        group_id: &GroupId,
        proposal: &MembershipProposal,
    ) -> Result<MembershipDecision, crate::MembershipActionError> {
        let rules = self.group_rules(group_id)?;
        Ok(aggregate_rule_decisions(rules, proposal))
    }

    fn apply_membership_decision(
        &mut self,
        group_id: &GroupId,
        proposal: MembershipProposal,
        decision: &MembershipDecision,
        now: u64,
    ) -> Result<(), crate::MembershipActionError> {
        let group = self
            .groups
            .get_mut(group_id)
            .ok_or_else(|| crate::MembershipActionError::GroupNotFound(group_id.clone()))?;

        if let Some(meta) = group.metadata.as_mut() {
            meta.updated_at = now;
        }

        match proposal.action {
            MembershipActionKind::Invite => match decision.status {
                MembershipDecisionStatus::Approved => {
                    let entry = group
                        .members
                        .entry(proposal.candidate.clone())
                        .or_insert_with(|| {
                            GroupMember::new(
                                proposal.candidate.clone(),
                                proposal.requested_role.clone(),
                                MembershipStatus::Active,
                                now,
                            )
                        });
                    entry.role_id = proposal.requested_role.clone();
                    entry.status = MembershipStatus::Active;
                    entry.last_activity = now;
                    group.membership_proposals.remove(&proposal.proposal_id);
                    sync_member_membership_sets(group, &proposal.candidate, now);
                }
                MembershipDecisionStatus::Pending => {
                    group
                        .membership_proposals
                        .insert(proposal.proposal_id.clone(), proposal.clone());
                    let entry = group
                        .members
                        .entry(proposal.candidate.clone())
                        .or_insert_with(|| {
                            GroupMember::new(
                                proposal.candidate.clone(),
                                proposal.requested_role.clone(),
                                MembershipStatus::Pending,
                                now,
                            )
                        });
                    entry.role_id = proposal.requested_role.clone();
                    entry.status = MembershipStatus::Pending;
                    entry.last_activity = now;
                    sync_member_membership_sets(group, &proposal.candidate, now);
                }
                MembershipDecisionStatus::Rejected => {
                    group.membership_proposals.remove(&proposal.proposal_id);
                    group.members.remove(&proposal.candidate);
                    sync_member_membership_sets(group, &proposal.candidate, now);
                }
            },
            MembershipActionKind::Remove => match decision.status {
                MembershipDecisionStatus::Approved => {
                    if let Some(member) = group.members.get_mut(&proposal.candidate) {
                        member.status = MembershipStatus::Removed;
                        member.last_activity = now;
                    }
                    group.membership_proposals.remove(&proposal.proposal_id);
                    sync_member_membership_sets(group, &proposal.candidate, now);
                }
                MembershipDecisionStatus::Pending => {
                    group
                        .membership_proposals
                        .insert(proposal.proposal_id.clone(), proposal.clone());
                }
                MembershipDecisionStatus::Rejected => {
                    group.membership_proposals.remove(&proposal.proposal_id);
                }
            },
        }

        Ok(())
    }

    fn process_membership_proposal(
        &mut self,
        group_id: &GroupId,
        proposal: MembershipProposal,
    ) -> Result<MembershipDecision, crate::MembershipActionError> {
        let decision = self.evaluate_membership(group_id, &proposal)?;
        let now = current_timestamp();
        self.apply_membership_decision(group_id, proposal, &decision, now)?;
        self.commit_group_crdt_or_log(group_id, "membership_proposal");
        Ok(decision)
    }
}
