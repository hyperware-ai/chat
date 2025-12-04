import React, { useEffect, useMemo, useState } from 'react';
import { Chat } from '#caller-utils';
import { useGroupStore } from '../../store/groups';
import { useChatStore } from '../../store/chat';
import { hasGroupPermission } from '../../constants/group';
import GroupMessage from './GroupMessage';
import GroupMessageInput from './GroupMessageInput';
import GroupStatusBar from './GroupStatusBar';
import GroupMembersModal from './GroupMembersModal';
import GroupReplicationPanel from './GroupReplicationPanel';
import GroupSettingsModal from './GroupSettingsModal';
import ThreadsList from './ThreadsList';
import ThreadView from './ThreadView';
import './GroupView.css';

type ThreadWithId = Chat.Thread & { id: string };

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
    whitelists,
    fetchWhitelist,
  } = useGroupStore();
  const { nodeId } = useChatStore();
  const [showMembers, setShowMembers] = useState(false);
  const [showSettings, setShowSettings] = useState(false);
  const [isFetchingWhitelist, setIsFetchingWhitelist] = useState(false);

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

  if (!activeGroup) return null;

  const threads = useMemo<ThreadWithId[]>(() => {
    const list = Array.from(activeGroup.threads.entries()).map(([id, thread]) => ({
      ...thread,
      id,
    }));
    return list.sort((a, b) => {
      const aTs = a.summary?.last_activity ?? 0;
      const bTs = b.summary?.last_activity ?? 0;
      if (aTs !== bTs) return bTs - aTs;
      return a.depth - b.depth;
    });
  }, [activeGroup.threads]);

  const selectedThreadId = activeThreadId || activeGroup.rootThreadId;
  useEffect(() => {
    if (!threads.length) return;
    const exists = threads.some((t) => t.id === selectedThreadId);
    if (!exists) {
      setActiveThread(threads[0].id);
    }
  }, [selectedThreadId, setActiveThread, threads]);

  const member = nodeId ? activeGroup.members.get(nodeId) : undefined;
  const role = member ? activeGroup.roles.get(member.role_id) : undefined;

  const canSend =
    member?.status === Chat.MembershipStatus.Active &&
    !!role &&
    hasGroupPermission(role.permissions as unknown as number, 'SEND_MESSAGES');
  const canCreateThread =
    member?.status === Chat.MembershipStatus.Active &&
    !!role &&
    hasGroupPermission(role.permissions as unknown as number, 'CREATE_THREADS');
  const canInvite =
    member?.status === Chat.MembershipStatus.Active &&
    !!role &&
    hasGroupPermission(role.permissions as unknown as number, 'INVITE_MEMBERS');
  const whitelist = whitelists[activeGroup.id];

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
    await createThread(title, activeGroup.rootThreadId);
  };

  const handleStartThreadFromMessage = async (parentThreadId: string) => {
    if (!canCreateThread) return;
    const title = window.prompt('Thread title (optional)') || null;
    await createThread(title, parentThreadId);
  };

  const handleFetchWhitelist = async () => {
    if (isFetchingWhitelist) return;
    setIsFetchingWhitelist(true);
    await fetchWhitelist(activeGroup.id);
    setIsFetchingWhitelist(false);
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
          <div className="group-dev-badge" aria-label="Development preview">
            Dev preview
          </div>
          <button className="group-settings" onClick={() => setShowSettings(true)}>
            Settings
          </button>
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

      <GroupReplicationPanel
        replication={replicationState}
        whitelist={whitelist}
        onRefresh={refreshActiveGroup}
        onFetchWhitelist={handleFetchWhitelist}
        isFetchingWhitelist={isFetchingWhitelist}
      />

      <section className="group-thread-section">
        <div className="group-thread-header">
          <div>
            <div className="section-title">Threads</div>
            <div className="section-sub">
              Fully separated from the main chat, sorted by recent activity.
            </div>
          </div>
          {canCreateThread && (
            <button className="thread-add" onClick={handleCreateThread}>
              + Thread
            </button>
          )}
        </div>

        <div className="group-thread-layout">
          <div className="thread-list-column">
            <ThreadsList
              threads={threads}
              activeThreadId={selectedThreadId}
              onSelect={(id) => setActiveThread(id)}
            />
          </div>
          <div className="thread-view-column">
            <ThreadView
              threadId={selectedThreadId}
              threads={threads}
              messages={activeGroup.messages}
              currentNode={nodeId}
              canSend={!!canSend}
              disabledReason={disabledReason}
              canStartThread={canCreateThread}
              onSend={sendMessage}
              onStartThread={canCreateThread ? handleStartThreadFromMessage : undefined}
              onOpenThread={(id) => setActiveThread(id)}
            />
          </div>
        </div>
      </section>

      {showMembers && <GroupMembersModal onClose={() => setShowMembers(false)} />}
      {showSettings && <GroupSettingsModal onClose={() => setShowSettings(false)} />}
    </div>
  );
};

export default GroupView;
