import { useEffect, useState } from 'react';
import './App.css';
import './styles/button-selectable.css';
import { useChatStore } from './store/chat';
import SplashScreen from './components/SplashScreen/SplashScreen';
import ChatView from './components/Chat/ChatView';
import { useGroupStore } from './store/groups';
import GroupView from './components/Groups/GroupView';
import GroupJoinModal from './components/Groups/GroupJoinModal';
import { parseGroupJoinLink } from './utils/groupLinks';
import type { GroupJoinTarget } from './utils/groupLinks';

function App() {
  const { 
    nodeId,
    isConnected,
    activeChat,
    error,
    initialize,
    clearError,
    chats,
    isLoading
  } = useChatStore();
  const { activeGroup, loadGroups, fetchReplicationState } = useGroupStore();
  const [pendingJoin, setPendingJoin] = useState<GroupJoinTarget | null>(null);

  // Initialize on mount
  useEffect(() => {
    initialize();
  }, [initialize]);

  // Prime group data when we have a connection
  useEffect(() => {
    if (isConnected) {
      loadGroups();
      fetchReplicationState(null);
    }
  }, [isConnected, loadGroups, fetchReplicationState]);

  useEffect(() => {
    const parsed = parseGroupJoinLink(window.location.pathname);
    if (parsed) {
      setPendingJoin(parsed);
      const parts = window.location.pathname.split('/').filter(Boolean);
      const joinIndex = parts.indexOf('join-group');
      if (joinIndex !== -1) {
        const baseParts = parts.slice(0, joinIndex);
        const basePath = baseParts.length ? `/${baseParts.join('/')}/` : '/';
        window.history.replaceState(
          {},
          '',
          `${basePath}${window.location.search}${window.location.hash}`,
        );
      }
    }
  }, []);

  useEffect(() => {
    const handleLinkClick = (event: MouseEvent) => {
      const target = event.target as HTMLElement | null;
      const anchor = target?.closest('a');
      const href = anchor?.getAttribute('href');
      if (!href) return;
      const parsed = parseGroupJoinLink(href);
      if (!parsed) return;
      event.preventDefault();
      event.stopPropagation();
      setPendingJoin(parsed);
    };
    document.addEventListener('click', handleLinkClick, true);
    return () => document.removeEventListener('click', handleLinkClick, true);
  }, []);

  // Show loading state only if we're truly loading (no cached data and no connection yet)
  // BUT: If we have chats from cache, skip the loading screen entirely
  if (chats.length === 0 && !nodeId && !error && isLoading) {
    return (
      <div className="app-loading">
        <div className="spinner" />
        <p>Connecting to Hyperware...</p>
      </div>
    );
  }

  // Show error only if we have no cached data AND there's a connection error
  if (!isConnected && error && chats.length === 0) {
    return (
      <div className="app-error">
        <h2>Connection Error</h2>
        <p>{error}</p>
        <button onClick={() => window.location.reload()}>
          Retry
        </button>
      </div>
    );
  }

  return (
    <div className="app">
      {/* Error banner */}
      {error && (
        <div className="error-banner">
          {error}
          <button onClick={clearError} className="dismiss-button">
            ×
          </button>
        </div>
      )}

      {/* Main app content */}
      {activeGroup ? <GroupView /> : activeChat ? <ChatView /> : <SplashScreen />}

      {pendingJoin && (
        <GroupJoinModal
          host={pendingJoin.host}
          keyValue={pendingJoin.key}
          onClose={() => setPendingJoin(null)}
        />
      )}
    </div>
  );
}

export default App;
