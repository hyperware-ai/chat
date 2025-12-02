import React, { useEffect, useMemo, useRef, useState } from 'react';
import { Chat } from '#caller-utils';
import { useGroupStore } from '../../store/groups';
import { useChatStore } from '../../store/chat';
import { hasGroupPermission } from '../../constants/group';
import GroupMessage from './GroupMessage';
import GroupMessageInput from './GroupMessageInput';
import GroupStatusBar from './GroupStatusBar';
import GroupMembersModal from './GroupMembersModal';
import './GroupView.css';

const GroupView: React.FC = () => {
  const {
    activeGroup,
    activeThreadId,
    setActiveThread,
    clearActiveGroup,
    createThread,
    sendMessage,
    refreshActiveGroup,
    subscriberEvents,
    replication,
    isSyncing,
    fetchSubscriberEvents,
  } = useGroupStore();
  const { nodeId } = useChatStore();
  const [showMembers, setShowMembers] = useState(false);
  const messagesEndRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!activeGroup) return;
    const interval = setInterval(() => refreshActiveGroup(), 15000);
    return () => clearInterval(interval);
  }, [activeGroup?.id, refreshActiveGroup]);

  useEffect(() => {
    const handleVisibility = () => {
      if (!document.hidden && activeGroup) {
        refreshActiveGroup();
      }
    };
    document.addEventListener('visibilitychange', handleVisibility);
    return () => document.removeEventListener('visibilitychange', handleVisibility);
  }, [activeGroup?.id, refreshActiveGroup]);

  useEffect(() => {
    if (!activeGroup) return;
    fetchSubscriberEvents();
    const interval = setInterval(() => fetchSubscriberEvents(), 20000);
    return () => clearInterval(interval);
  }, [activeGroup?.id, fetchSubscriberEvents]);

  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [activeGroup?.messages.length, activeThreadId]);

  if (!activeGroup) return null;

  const threads = useMemo(() => {
    const list = Array.from(activeGroup.threads.entries()).map(([id, thread]) => ({
      ...thread,
      id,
    }));
    return list.sort(
      (a, b) => (b.summary?.last_activity ?? 0) - (a.summary?.last_activity ?? 0),
    );
  }, [activeGroup.threads]);

  const selectedThreadId = activeThreadId || activeGroup.rootThreadId;
  const threadMessages = selectedThreadId
    ? activeGroup.messages.filter((msg) => msg.threadId === selectedThreadId)
    : activeGroup.messages;

  const member = nodeId ? activeGroup.members.get(nodeId) : undefined;
  const role = member ? activeGroup.roles.get(member.role_id) : undefined;

  const canSend =
    member?.status === Chat.MembershipStatus.Active &&
    role &&
    hasGroupPermission(role.permissions as unknown as number, 'SEND_MESSAGES');
  const canCreateThread =
    member?.status === Chat.MembershipStatus.Active &&
    role &&
    hasGroupPermission(role.permissions as unknown as number, 'CREATE_THREADS');
  const canInvite =
    member?.status === Chat.MembershipStatus.Active &&
    role &&
    hasGroupPermission(role.permissions as unknown as number, 'INVITE_MEMBERS');

  let disabledReason: string | undefined;
  if (!member) disabledReason = 'You are not a member of this group.';
  else if (member.status !== Chat.MembershipStatus.Active)
    disabledReason = 'Membership pending approval.';
  else if (!canSend)
    disabledReason = 'You do not have permission to post.';

  const groupEvents = subscriberEvents.filter(
    (evt) => evt.group_id === activeGroup.id,
  );
  const replicationState = replication[activeGroup.id];

  const handleCreateThread = async () => {
    if (!canCreateThread) return;
    const title = window.prompt('Thread title (optional)') || null;
    await createThread(title);
  };

  return (
    <div className="group-view">
      <header className="group-header">
        <button className="group-back" onClick={clearActiveGroup}>
          ←
        </button>
        <div className="group-header-info">
          <div className="group-header-name">{activeGroup.metadata.name}</div>
          <div className="group-header-sub">
            <span>{activeGroup.members.size} members</span>
            <span className="dot">•</span>
            <span>{threads.length} threads</span>
            {role && (
              <>
                <span className="dot">•</span>
                <span>Role: {role.label}</span>
              </>
            )}
          </div>
        </div>
        <div className="group-header-actions">
          <button className="group-members" onClick={() => setShowMembers(true)}>
            Members
          </button>
          <button className="group-sync" onClick={refreshActiveGroup}>
            {isSyncing ? 'Syncing…' : 'Sync'}
          </button>
        </div>
      </header>

      <GroupStatusBar
        replication={replicationState}
        events={groupEvents}
        onRefresh={refreshActiveGroup}
        isSyncing={isSyncing}
      />

      <div className="group-thread-bar">
        <div className="group-thread-chips">
          {threads.map((thread) => (
            <button
              key={thread.id}
              className={`thread-chip ${
                thread.id === activeThreadId ? 'active' : ''
              }`}
              onClick={() => setActiveThread(thread.id)}
            >
              <span className="thread-title">
                {thread.title || (thread.depth === 0 ? 'Main thread' : thread.id)}
              </span>
              <span className="thread-meta">
                {thread.summary?.message_count ?? 0} msgs
              </span>
            </button>
          ))}
        </div>
        {canCreateThread && (
          <button className="thread-add" onClick={handleCreateThread}>
            + Thread
          </button>
        )}
      </div>

      <div className="group-messages">
        {threadMessages.length === 0 ? (
          <div className="group-empty-thread">
            No messages yet. {canSend ? 'Start the conversation!' : 'Waiting for others.'}
          </div>
        ) : (
          threadMessages.map((msg) => (
            <GroupMessage key={msg.id} message={msg} currentNode={nodeId} />
          ))
        )}
        <div ref={messagesEndRef} />
      </div>

      <GroupMessageInput
        onSend={sendMessage}
        disabled={!canSend}
        disabledReason={disabledReason}
      />

      {showMembers && <GroupMembersModal onClose={() => setShowMembers(false)} />}
    </div>
  );
};

export default GroupView;
