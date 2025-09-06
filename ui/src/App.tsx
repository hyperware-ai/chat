import { useEffect } from 'react';
import './App.css';
import { useChatStore } from './store/chat';
import SplashScreen from './components/SplashScreen/SplashScreen';
import ChatView from './components/Chat/ChatView';

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

  // Initialize on mount
  useEffect(() => {
    initialize();
  }, [initialize]);

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
      {activeChat ? (
        <ChatView />
      ) : (
        <SplashScreen />
      )}
    </div>
  );
}

export default App;