import React, { useEffect, useMemo, useRef, useState } from 'react';
import { useChatStore } from '../../store/chat';
import { useGroupStore } from '../../store/groups';
import ChatSearch from '../Chats/ChatSearch';
import NewChatModal from '../Chats/NewChatModal';
import GroupCreateModal from '../Groups/GroupCreateModal';
import './UnifiedMessages.css';

const UnifiedMessages: React.FC = () => {
  const { chats, searchChats, connectionStatus, setActiveChat } = useChatStore();
  const {
    groups,
    groupPreviews,
    groupUnread,
    loadGroups,
    fetchReplicationState,
    openGroup,
    isLoading,
    error,
  } = useGroupStore();

  const [query, setQuery] = useState('');
  const [dmResults, setDmResults] = useState(chats);
  const [showNewChat, setShowNewChat] = useState(false);
  const [showCreateGroup, setShowCreateGroup] = useState(false);
  const [showChooser, setShowChooser] = useState(false);
  const chooserOpenedAtRef = useRef(0);

  // Keep group data fresh when we land on the unified view
  useEffect(() => {
    if (connectionStatus === 'connected') {
      loadGroups();
      fetchReplicationState(null);
    }
  }, [connectionStatus, loadGroups, fetchReplicationState]);

  // Filter DMs using the server-side search helper
  useEffect(() => {
    let cancelled = false;

    const runSearch = async () => {
      try {
        if (query) {
          const results = await searchChats(query);
          if (!cancelled) setDmResults(results);
        } else {
          setDmResults(chats);
        }
      } catch (err) {
        if (!cancelled) setDmResults([]);
      }
    };

    runSearch();
    return () => {
      cancelled = true;
    };
  }, [query, chats, searchChats]);

  const filteredGroups = useMemo(() => {
    if (!query) return groups;
    const term = query.toLowerCase();
    return groups.filter((group) => {
      const name = group.metadata?.name?.toLowerCase() || '';
      const desc = group.metadata?.description?.toLowerCase() || '';
      return name.includes(term) || desc.includes(term);
    });
  }, [groups, query]);

  const unifiedItems = useMemo(() => {
    const dmItems = dmResults.map(chat => {
      const lastMessage = chat.messages[chat.messages.length - 1];
      const lastActivity = chat.last_activity || lastMessage?.timestamp || 0;
      const preview = lastMessage?.content || 'No messages yet';
      return {
        id: `dm-${chat.id}`,
        kind: 'dm' as const,
        title: chat.counterparty || 'Direct message',
        subtitle: preview,
        threadPath: null as string | null,
        lastActivity,
        unread: chat.unread_count,
        isOfficial: chat.counterparty === 'dao.hypr',
        meta: undefined,
        onClick: () => setActiveChat(chat),
      };
    });

    const groupItems = filteredGroups.map((group) => {
      const preview = groupPreviews[group.group_id];
      const lastActivity =
        preview?.timestamp || group.metadata?.updated_at || Math.floor(Date.now() / 1000);
      const subtitle = preview?.text || 'No messages yet';
      const threadPath = preview?.threadPath || null;
      return {
        id: `group-${group.group_id}`,
        kind: 'group' as const,
        title: group.metadata?.name || 'Untitled group',
        subtitle,
        threadPath,
        lastActivity,
        onClick: () => openGroup(group.group_id),
        unread: groupUnread[group.group_id] || 0,
      };
    });

    return [...dmItems, ...groupItems].sort(
      (a, b) => (b.lastActivity || 0) - (a.lastActivity || 0)
    );
  }, [dmResults, filteredGroups, openGroup, setActiveChat, groupPreviews]);

  const formatTime = (timestamp?: number | null) => {
    if (!timestamp) return '';
    const date = new Date(timestamp * 1000);
    const now = new Date();
    const diff = now.getTime() - date.getTime();
    const days = Math.floor(diff / (1000 * 60 * 60 * 24));
    
    if (days === 0) {
      return date.toLocaleTimeString('en-US', { hour: '2-digit', minute: '2-digit' });
    } else if (days === 1) {
      return 'Yesterday';
    } else if (days < 7) {
      return date.toLocaleDateString('en-US', { weekday: 'short' });
    } else {
      return date.toLocaleDateString('en-US', { month: 'short', day: 'numeric' });
    }
  };

  useEffect(() => {
    if (showChooser) {
      chooserOpenedAtRef.current = Date.now();
    }
  }, [showChooser]);

  const handleChooserOverlayClick = (event: React.MouseEvent<HTMLDivElement>) => {
    // Ignore the delayed click that can land on the overlay right after opening on Android.
    if (Date.now() - chooserOpenedAtRef.current < 350) {
      event.stopPropagation();
      return;
    }
    setShowChooser(false);
  };

  const NewChatChooser = () => (
    <div className="modal-overlay" onClick={handleChooserOverlayClick}>
      <div className="modal-content chooser" onClick={(e) => e.stopPropagation()}>
        <div className="modal-header">
          <h3>New chat</h3>
          <button className="close-button" onClick={() => setShowChooser(false)}>
            ×
          </button>
        </div>
        <div className="chooser-actions">
          <button
            className="chooser-action"
            onClick={() => {
              setShowChooser(false);
              setShowCreateGroup(true);
            }}
          >
            <span className="chooser-icon">👥</span>
            <div>
              <div className="chooser-title">Create Group Chat</div>
              <div className="chooser-subtitle">Name it and invite members</div>
            </div>
          </button>
          <button
            className="chooser-action"
            onClick={() => {
              setShowChooser(false);
              setShowNewChat(true);
            }}
          >
            <span className="chooser-icon">💬</span>
            <div>
              <div className="chooser-title">Start One-on-One</div>
              <div className="chooser-subtitle">Message a single user</div>
            </div>
          </button>
        </div>
      </div>
    </div>
  );

  return (
    <div className="unified-messages">
      <div className="unified-toolbar">
        <ChatSearch
          value={query}
          onChange={setQuery}
          placeholder="Search DMs or groups..."
        />
        <div className="unified-actions">
          <button
            className="unified-action primary"
            onClick={() => setShowChooser(true)}
            aria-label="New chat"
          >
            + New
          </button>
        </div>
      </div>

      {error && <div className="unified-error">{error}</div>}

      <div className="unified-scroll">
        <section className="unified-section">
          <div className="unified-list">
            {isLoading && unifiedItems.length === 0 ? (
              <div className="unified-empty">Loading chats…</div>
            ) : unifiedItems.length ? (
              unifiedItems.map((item) => (
                <button
                  key={item.id}
                  className={`unified-item ${item.kind} ${'isOfficial' in item && item.isOfficial ? 'official-chat' : ''}`}
                  onClick={item.onClick}
                >
                  <div className="unified-avatar" aria-hidden="true">
                    {item.title.slice(0, 2).toUpperCase()}
                  </div>
                  <div className="unified-item-body">
                    <div className="unified-item-row">
                      <div className="unified-item-title">
                        {item.title}
                        {'isOfficial' in item && item.isOfficial ? (
                          <span className="official-badge">official</span>
                        ) : null}
                      </div>
                      <div className="unified-item-meta">
                        {item.lastActivity ? (
                          <span className="unified-time">
                            {formatTime(item.lastActivity)}
                          </span>
                        ) : null}
                      </div>
                    </div>
                    <div className="unified-item-row secondary">
                      <div className="unified-item-subtitle">
                        {item.threadPath && (
                          <span className="unified-thread-path">{item.threadPath}: </span>
                        )}
                        {item.subtitle}
                      </div>
                      {'unread' in item && item.unread ? (
                        <span className="unified-unread">{item.unread}</span>
                      ) : null}
                    </div>
                  </div>
                </button>
              ))
            ) : (
              <div className="unified-empty">
                {query
                  ? 'No chats match your search.'
                  : 'No conversations yet. Start a DM or create a group.'}
              </div>
            )}
          </div>
        </section>
      </div>

      {showNewChat && <NewChatModal onClose={() => setShowNewChat(false)} />}
      {showCreateGroup && (
        <GroupCreateModal onClose={() => setShowCreateGroup(false)} />
      )}
      {showChooser && <NewChatChooser />}
    </div>
  );
};

export default UnifiedMessages;
