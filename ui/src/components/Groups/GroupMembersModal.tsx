import React, { useEffect, useMemo, useState } from 'react';
import { Chat } from '#caller-utils';
import { useGroupStore } from '../../store/groups';
import { hasGroupPermission } from '../../constants/group';
import './GroupMembersModal.css';

interface GroupMembersModalProps {
  onClose: () => void;
}

const statusLabel = (status: Chat.MembershipStatus) => {
  switch (status) {
    case Chat.MembershipStatus.Active:
      return 'Active';
    case Chat.MembershipStatus.Pending:
      return 'Pending';
    case Chat.MembershipStatus.Removed:
      return 'Removed';
    default:
      return status;
  }
};

const formatLastSeen = (ts: number) => {
  if (!ts) return '—';
  const date = new Date(ts * 1000);
  const diff = Date.now() - date.getTime();
  const minutes = Math.floor(diff / 60000);
  if (minutes < 1) return 'Just now';
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  return date.toLocaleDateString();
};

/**
 * Extract required signers from membership rules.
 * For dictator rule: returns the dictator
 * For multi_dictator rule: returns all dictators
 * For other rules: returns empty array (they use different approval mechanisms)
 */
const getRequiredSigners = (rules: Chat.MembershipRuleConfig[]): string[] => {
  const signers: string[] = [];
  for (const rule of rules) {
    if (rule.rule_id === 'membership.rule.dictator') {
      const params = rule.params as { dictator?: string };
      if (params?.dictator) {
        signers.push(params.dictator);
      }
    } else if (rule.rule_id === 'membership.rule.multi_dictator') {
      const params = rule.params as { dictators?: string[] };
      if (params?.dictators) {
        signers.push(...params.dictators);
      }
    }
  }
  return signers;
};

/**
 * Check if the group is currently in solo dictatorship mode.
 * Returns true if there's exactly one dictator rule with a single dictator.
 */
const isSoloDictatorship = (rules: Chat.MembershipRuleConfig[]): boolean => {
  if (rules.length !== 1) return false;
  return rules[0].rule_id === 'membership.rule.dictator';
};

const GroupMembersModal: React.FC<GroupMembersModalProps> = ({ onClose }) => {
  const {
    activeGroup,
    inviteMember,
    approveProposal,
    refreshActiveGroup,
    removeMember,
    leaveGroup,
    createGroupJoinLink,
    updateGroupVisibility,
  } = useGroupStore();
  const [candidate, setCandidate] = useState('');
  const [roleId, setRoleId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [busyMember, setBusyMember] = useState<string | null>(null);
  const [leaveBusy, setLeaveBusy] = useState(false);
  const [joinLink, setJoinLink] = useState<string | null>(null);
  const [joinError, setJoinError] = useState<string | null>(null);
  const [isCreatingLink, setIsCreatingLink] = useState(false);
  const [isUpdatingVisibility, setIsUpdatingVisibility] = useState(false);
  const [visibilityError, setVisibilityError] = useState<string | null>(null);
  const [isConfirmingVisibility, setIsConfirmingVisibility] = useState(false);
  const [pendingVisibility, setPendingVisibility] = useState<Chat.GroupVisibility | null>(null);

  const currentNode = (window as any).our?.node || null;
  if (!activeGroup) return null;

  const me = currentNode ? activeGroup.members.get(currentNode) : undefined;
  const myRole = me ? activeGroup.roles.get(me.role_id) : undefined;
  const canInvite =
    me?.status === Chat.MembershipStatus.Active &&
    myRole &&
    hasGroupPermission(myRole.permissions as unknown as number, 'INVITE_MEMBERS');
  const canManageSettings =
    me?.status === Chat.MembershipStatus.Active &&
    myRole &&
    hasGroupPermission(myRole.permissions as unknown as number, 'MANAGE_SETTINGS');
  const canRemove = canInvite;
  const isPublic = activeGroup.metadata.visibility === Chat.GroupVisibility.Public;
  const visibilityLabel = isPublic ? 'Public' : 'Private';

  const roles = useMemo(() => Array.from(activeGroup.roles.values()), [activeGroup.roles]);
  const defaultRoleId =
    roleId ||
    activeGroup.metadata.default_role_id ||
    roles.find((r) => r.tier === Chat.GroupTier.Subscriber)?.id ||
    roles[0]?.id ||
    '';

  const sortedMembers = useMemo(() => {
    return Array.from(activeGroup.members.entries())
      .filter(([, member]) => member.status !== Chat.MembershipStatus.Removed)
      .sort(([, a], [, b]) => {
        const tsA = a.last_activity;
        const tsB = b.last_activity;
        return tsB - tsA;
      });
  }, [activeGroup.members]);

  const pendingProposals = activeGroup.proposals;
  const requiredSigners = useMemo(
    () => getRequiredSigners(activeGroup.membershipRules),
    [activeGroup.membershipRules]
  );
  const canApprove = currentNode && requiredSigners.includes(currentNode);

  // Check if selected role is Hub tier (Owner) and group is solo dictatorship
  const selectedRole = roles.find((r) => r.id === defaultRoleId);
  const isSelectingOwner = selectedRole?.tier === Chat.GroupTier.Hub;
  const showOwnerWarning = isSelectingOwner && isSoloDictatorship(activeGroup.membershipRules);

  const handleInvite = async () => {
    if (!canInvite) {
      setError('You do not have permission to invite members.');
      return;
    }
    if (!candidate.trim()) {
      setError('Enter a node ID to invite.');
      return;
    }
    setIsSubmitting(true);
    setError(null);
    const decision = await inviteMember(candidate.trim(), defaultRoleId);
    setIsSubmitting(false);
    if (decision) {
      setCandidate('');
      await refreshActiveGroup();
    } else {
      setError('Invite failed. Check permissions or try again.');
    }
  };

  const handleApprove = async (proposalId: string) => {
    await approveProposal(proposalId);
  };

  const handleRemove = async (member: string) => {
    if (!canRemove) {
      setError('You do not have permission to manage members.');
      return;
    }
    setBusyMember(member);
    const decision = await removeMember(member);
    if (!decision) {
      setError('Unable to remove member. Try again.');
    }
    setBusyMember(null);
  };

  const handleLeave = async () => {
    if (!currentNode) return;
    setLeaveBusy(true);
    const decision = await leaveGroup();
    if (!decision) {
      setError('Unable to leave group. Try again.');
      setLeaveBusy(false);
      return;
    }
    setLeaveBusy(false);
    // leaveGroup already clears active group and removes from list
    onClose();
  };

  const handleCreateLink = async () => {
    if (!isPublic || !canInvite) return;
    setIsCreatingLink(true);
    setJoinError(null);
    const link = await createGroupJoinLink(activeGroup.id);
    if (link) {
      setJoinLink(link);
    } else {
      setJoinError('Failed to create join link.');
    }
    setIsCreatingLink(false);
  };

  useEffect(() => {
    if (!isPublic) {
      setJoinLink(null);
      setJoinError(null);
    }
  }, [isPublic]);

  const handleVisibilityChange = async (visibility: Chat.GroupVisibility) => {
    if (!canManageSettings || isUpdatingVisibility) return;
    if (visibility === activeGroup.metadata.visibility) return;
    setIsUpdatingVisibility(true);
    setVisibilityError(null);
    const ok = await updateGroupVisibility(activeGroup.id, visibility);
    if (!ok) {
      setVisibilityError('Failed to update visibility.');
    } else if (visibility === Chat.GroupVisibility.Private) {
      setJoinLink(null);
      setJoinError(null);
    }
    setIsUpdatingVisibility(false);
  };

  const requestVisibilityChange = (visibility: Chat.GroupVisibility) => {
    if (!canManageSettings || isUpdatingVisibility) return;
    setPendingVisibility(visibility);
    setIsConfirmingVisibility(true);
  };

  const confirmVisibilityChange = async () => {
    if (!pendingVisibility) return;
    await handleVisibilityChange(pendingVisibility);
    setIsConfirmingVisibility(false);
    setPendingVisibility(null);
  };

  const cancelVisibilityChange = () => {
    setIsConfirmingVisibility(false);
    setPendingVisibility(null);
  };

  const handleCopyLink = () => {
    if (!joinLink) return;
    navigator.clipboard.writeText(joinLink).catch(() => {});
  };

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div className="modal-content members-modal" onClick={(e) => e.stopPropagation()}>
        <div className="modal-header">
          <h3>Members</h3>
          <button className="close-button" onClick={onClose}>
            ×
          </button>
        </div>

        <div className="modal-body members-body">
          <div className="members-section">
            <div className="members-section-header">
              <h4>People ({sortedMembers.length})</h4>
            </div>
            <div className="members-list">
              {sortedMembers.map(([node, member]) => {
                const role = activeGroup.roles.get(member.role_id);
                return (
                  <div key={node} className="member-row">
                    <div className="member-primary">
                      <div className="member-node">{node}</div>
                      <div className="member-sub">
                        <span>{role?.label || 'Member'}</span>
                        <span className="dot">•</span>
                        <span>{statusLabel(member.status)}</span>
                        {role?.tier && (
                          <>
                            <span className="dot">•</span>
                            <span>{role.tier === Chat.GroupTier.Hub ? 'Hub' : 'Subscriber'}</span>
                          </>
                        )}
                      </div>
                    </div>
                    <div className="member-meta">
                      <span>Last seen {formatLastSeen(member.last_activity)}</span>
                      <div className="member-actions">
                        {currentNode === node ? (
                          <button
                            className="danger"
                            onClick={handleLeave}
                            disabled={leaveBusy}
                          >
                            {leaveBusy ? 'Leaving…' : 'Leave group'}
                          </button>
                        ) : (
                          <button
                            className="secondary danger"
                            onClick={() => handleRemove(node)}
                            disabled={!canRemove || busyMember === node}
                          >
                            {busyMember === node ? 'Removing…' : 'Remove'}
                          </button>
                        )}
                      </div>
                    </div>
                  </div>
                );
              })}
            </div>
          </div>

          {canInvite && (
            <div className="members-section">
              <div className="members-section-header">
                <h4>Invite</h4>
              </div>
              <div className="invite-form">
                <input
                  type="text"
                  placeholder="Node ID (e.g., alice.node)"
                  value={candidate}
                  onChange={(e) => setCandidate(e.target.value)}
                />
                <select
                  value={defaultRoleId}
                  onChange={(e) => setRoleId(e.target.value)}
                >
                  {roles.map((role) => (
                    <option key={role.id} value={role.id}>
                      {role.label}
                    </option>
                  ))}
                </select>
                {showOwnerWarning && (
                  <div className="owner-warning">
                    Adding another Owner will convert this group to a multi-dictatorship. You will share full control of the group with them.
                  </div>
                )}
                <button
                  className="primary"
                  onClick={handleInvite}
                  disabled={isSubmitting}
                >
                  {isSubmitting ? 'Inviting…' : 'Send invite'}
                </button>
                {error && <div className="members-error">{error}</div>}
              </div>
            </div>
          )}

          {pendingProposals.length > 0 && (
            <div className="members-section">
              <div className="members-section-header">
                <h4>Pending approvals</h4>
              </div>
              <div className="pending-list">
                {pendingProposals.map((proposal) => {
                  const role = activeGroup.roles.get(proposal.requested_role);
                  // Filter out signers who have already approved
                  const pendingSigners = requiredSigners.filter(
                    (s) => !proposal.approvals?.includes(s)
                  );
                  const isRemoval = proposal.action === Chat.MembershipActionKind.Remove;
                  const actionLabel = isRemoval ? 'Remove' : 'Add';
                  const actionDescription = isRemoval
                    ? `removing ${proposal.candidate}`
                    : `adding ${proposal.candidate}`;
                  return (
                    <div key={proposal.proposal_id} className="pending-row">
                      <div className="pending-main">
                        <div className="pending-node">
                          {actionLabel}: {proposal.candidate}
                        </div>
                        <div className="pending-sub">
                          {isRemoval
                            ? `Proposed by ${proposal.proposer}`
                            : `Role: ${role?.label || proposal.requested_role}`}
                        </div>
                      </div>
                      {canApprove ? (
                        <button
                          className={isRemoval ? 'secondary danger' : 'secondary'}
                          onClick={() => handleApprove(proposal.proposal_id)}
                          disabled={!canInvite}
                        >
                          Approve {actionLabel}
                        </button>
                      ) : (
                        <div className="pending-status">
                          Awaiting approval from {pendingSigners.join(', ') || 'authorized members'} for {actionDescription}
                        </div>
                      )}
                    </div>
                  );
                })}
              </div>
            </div>
          )}

          <div className="members-section">
            <div className="members-section-header">
              <h4>Visibility: {visibilityLabel}</h4>
            </div>
            <div className="members-visibility-controls">
              <div className="members-visibility-info">
                <div className="members-visibility-title">Group visibility</div>
                <div className="members-visibility-desc">
                  {isPublic
                    ? 'Anyone with the join link can join immediately.'
                    : 'Private groups are invite-only.'}
                </div>
              </div>
              <button
                type="button"
                className="visibility-action-button"
                onClick={() =>
                  requestVisibilityChange(
                    isPublic ? Chat.GroupVisibility.Private : Chat.GroupVisibility.Public,
                  )
                }
                disabled={!canManageSettings || isUpdatingVisibility}
              >
                {isPublic ? 'Change visibility to private' : 'Change visibility to public'}
              </button>
            </div>
            {!canManageSettings && (
              <div className="members-visibility-note">
                You do not have permission to change visibility.
              </div>
            )}
            {isUpdatingVisibility && (
              <div className="members-visibility-note">Updating visibility…</div>
            )}
            {visibilityError && <div className="members-error">{visibilityError}</div>}
            {isPublic && (
              <div className="members-join-link">
                <div className="members-join-header">
                  <h4>Join link</h4>
                  <span className="members-hint">
                    Anyone with this link can join as a member.
                  </span>
                </div>
                <div className="join-link-actions">
                  <div className="join-link-row">
                    <button
                      className="join-link-button"
                      onClick={handleCreateLink}
                      disabled={!canInvite || isCreatingLink}
                    >
                      {isCreatingLink ? 'Creating…' : 'Create join link'}
                    </button>
                    {!canInvite && (
                      <span className="join-link-note">
                        You do not have permission to create join links.
                      </span>
                    )}
                  </div>
                  {joinLink && (
                    <div className="join-link-display">
                      <input
                        type="text"
                        value={joinLink}
                        readOnly
                        onClick={(e) => (e.target as HTMLInputElement).select()}
                      />
                      <button className="join-link-button secondary" onClick={handleCopyLink}>
                        Copy
                      </button>
                    </div>
                  )}
                  {joinError && <div className="join-link-error">{joinError}</div>}
                </div>
              </div>
            )}
          </div>
        </div>
      </div>
      {isConfirmingVisibility && pendingVisibility && (
        <div
          className="members-confirm-overlay"
          onClick={(e) => {
            e.stopPropagation();
            cancelVisibilityChange();
          }}
        >
          <div className="members-confirm-modal" onClick={(e) => e.stopPropagation()}>
            <div className="members-confirm-title">Are you sure?</div>
            <div className="members-confirm-body">
              Change visibility to{' '}
              {pendingVisibility === Chat.GroupVisibility.Public ? 'public' : 'private'}?
            </div>
            <div className="members-confirm-actions">
              <button className="join-link-button secondary" onClick={cancelVisibilityChange}>
                Cancel
              </button>
              <button className="join-link-button" onClick={confirmVisibilityChange}>
                Confirm
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
};

export default GroupMembersModal;
