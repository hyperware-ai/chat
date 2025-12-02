import React, { useEffect, useMemo, useState } from 'react';
import { useGroupStore } from '../../store/groups';
import { useChatStore } from '../../store/chat';
import GroupListItem from './GroupListItem';
import GroupCreateModal from './GroupCreateModal';
import './GroupList.css';

const GroupList: React.FC = () => {
  const {
    groups,
    loadGroups,
    openGroup,
    activeGroupId,
    replication,
    isLoading,
    error,
    fetchReplicationState,
  } = useGroupStore();
  const { connectionStatus } = useChatStore();
  const [search, setSearch] = useState('');
  const [showCreate, setShowCreate] = useState(false);

  useEffect(() => {
    if (connectionStatus === 'connected') {
      loadGroups();
      fetchReplicationState(null);
    }
  }, [connectionStatus]);

  const filteredGroups = useMemo(() => {
    if (!search) return groups;
    const term = search.toLowerCase();
    return groups.filter((group) => {
      const name = group.metadata?.name?.toLowerCase() || '';
      const desc = group.metadata?.description?.toLowerCase() || '';
      return name.includes(term) || desc.includes(term);
    });
  }, [groups, search]);

  return (
    <div className="group-list-container">
      <div className="group-list-header">
        <div className="group-search">
          <input
            type="text"
            placeholder="Find a group..."
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />
        </div>
        <button
          className="group-refresh"
          onClick={() => {
            loadGroups();
            fetchReplicationState(null);
          }}
          disabled={connectionStatus !== 'connected'}
        >
          ⟳
        </button>
        <button
          className="group-create"
          onClick={() => setShowCreate(true)}
          disabled={connectionStatus !== 'connected'}
        >
          + New
        </button>
      </div>

      {error && <div className="group-error-banner">{error}</div>}

      <div className="group-list">
        {isLoading && groups.length === 0 ? (
          <div className="group-empty">Loading groups…</div>
        ) : filteredGroups.length > 0 ? (
          filteredGroups.map((group) => (
            <GroupListItem
              key={group.group_id}
              summary={group}
              replicationState={replication[group.group_id]}
              isActive={group.group_id === activeGroupId}
              onSelect={(id) => openGroup(id)}
            />
          ))
        ) : (
          <div className="group-empty">
            {search ? 'No groups match your search.' : 'No groups yet. Create one to get started.'}
          </div>
        )}
      </div>

      {showCreate && <GroupCreateModal onClose={() => setShowCreate(false)} />}
    </div>
  );
};

export default GroupList;
