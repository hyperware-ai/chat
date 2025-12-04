import React, { useState } from 'react';
import TabBar from './TabBar';
import ProfileButton from './ProfileButton';
import CallHistory from '../Calls/CallHistory';
import SettingsModal from '../Settings/SettingsModal';
import UnifiedMessages from './UnifiedMessages';
import './SplashScreen.css';

type TabType = 'chats' | 'calls';

const SplashScreen: React.FC = () => {
  const [activeTab, setActiveTab] = useState<TabType>('chats');
  const [showSettings, setShowSettings] = useState(false);

  const renderContent = () => {
    return activeTab === 'calls' ? <CallHistory /> : <UnifiedMessages />;
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
      
      <TabBar
        activeTab={activeTab}
        onTabChange={setActiveTab}
      />
      
      {showSettings && (
        <SettingsModal onClose={() => setShowSettings(false)} />
      )}
    </div>
  );
};

export default SplashScreen;
