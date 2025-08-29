import React, { useState } from 'react';
import TabBar from './TabBar';
import ProfileButton from './ProfileButton';
import ChatList from '../Chats/ChatList';
import GroupList from '../Groups/GroupList';
import CallHistory from '../Calls/CallHistory';
import SettingsModal from '../Settings/SettingsModal';
import './SplashScreen.css';

type TabType = 'chats' | 'groups' | 'calls';

const SplashScreen: React.FC = () => {
  const [activeTab, setActiveTab] = useState<TabType>('chats');
  const [showSettings, setShowSettings] = useState(false);

  const renderContent = () => {
    switch (activeTab) {
      case 'chats':
        return <ChatList />;
      case 'groups':
        return <GroupList />;
      case 'calls':
        return <CallHistory />;
      default:
        return <ChatList />;
    }
  };

  return (
    <div className="splash-screen">
      <div className="splash-header">
        <ProfileButton onClick={() => setShowSettings(true)} />
        <h1 className="app-title">Chat</h1>
        <div className="header-spacer" />
      </div>
      
      <div className="splash-content">
        {renderContent()}
      </div>
      
      <TabBar activeTab={activeTab} onTabChange={setActiveTab} />
      
      {showSettings && (
        <SettingsModal onClose={() => setShowSettings(false)} />
      )}
    </div>
  );
};

export default SplashScreen;