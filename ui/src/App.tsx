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
    clearError 
  } = useChatStore();

  // Initialize on mount
  useEffect(() => {
    initialize();
  }, [initialize]);

  // Show loading state while connecting
  if (!nodeId && !error) {
    return (
      <div className="app-loading">
        <div className="spinner" />
        <p>Connecting to Hyperware...</p>
      </div>
    );
  }

  // Show error if not connected
  if (!isConnected && error) {
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