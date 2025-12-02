import React, { useMemo, useState } from 'react';
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

const GroupMembersModal: React.FC<GroupMembersModalProps> = ({ onClose }) => {
  const {
    activeGroup,
    inviteMember,
    approveProposal,
    refreshActiveGroup,
  } = useGroupStore();
  const [candidate, setCandidate] = useState('');
  const [roleId, setRoleId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [isSubmitting, setIsSubmitting] = useState(false);

  const currentNode = (window as any).our?.node || null;
  if (!activeGroup) return null;

  const me = currentNode ? activeGroup.members.get(currentNode) : undefined;
  const myRole = me ? activeGroup.roles.get(me.role_id) : undefined;
  const canInvite =
    me?.status === Chat.MembershipStatus.Active &&
    myRole &&
    hasGroupPermission(myRole.permissions as unknown as number, 'INVITE_MEMBERS');

  const roles = useMemo(() => Array.from(activeGroup.roles.values()), [activeGroup.roles]);
  const defaultRoleId =
    roleId ||
    activeGroup.metadata.default_role_id ||
    roles.find((r) => r.tier === Chat.GroupTier.Subscriber)?.id ||
    roles[0]?.id ||
    '';

  const sortedMembers = useMemo(() => {
    return Array.from(activeGroup.members.entries()).sort(([, a], [, b]) => {
      const tsA = a.last_activity;
      const tsB = b.last_activity;
      return tsB - tsA;
    });
  }, [activeGroup.members]);

  const pendingProposals = activeGroup.proposals;

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
              <h4>People ({activeGroup.members.size})</h4>
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
                      </div>
                    </div>
                    <div className="member-meta">
                      <span>Last seen {formatLastSeen(member.last_activity)}</span>
                    </div>
                  </div>
                );
              })}
            </div>
          </div>

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
              <button
                className="primary"
                onClick={handleInvite}
                disabled={!canInvite || isSubmitting}
              >
                {isSubmitting ? 'Inviting…' : 'Send invite'}
              </button>
              {error && <div className="members-error">{error}</div>}
              {!canInvite && (
                <div className="members-hint">
                  You need invite permissions to add people.
                </div>
              )}
            </div>
          </div>

          {pendingProposals.length > 0 && (
            <div className="members-section">
              <div className="members-section-header">
                <h4>Pending approvals</h4>
              </div>
              <div className="pending-list">
                {pendingProposals.map((proposal) => {
                  const role = activeGroup.roles.get(proposal.requested_role);
                  return (
                    <div key={proposal.proposal_id} className="pending-row">
                      <div className="pending-main">
                        <div className="pending-node">{proposal.candidate}</div>
                        <div className="pending-sub">
                          Requested: {role?.label || proposal.requested_role}
                        </div>
                      </div>
                      <button
                        className="secondary"
                        onClick={() => handleApprove(proposal.proposal_id)}
                        disabled={!canInvite}
                      >
                        Approve
                      </button>
                    </div>
                  );
                })}
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  );
};

export default GroupMembersModal;
