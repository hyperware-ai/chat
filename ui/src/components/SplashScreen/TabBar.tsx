import React from 'react';
import './TabBar.css';

interface TabBarProps {
  activeTab: 'chats' | 'groups' | 'calls';
  onTabChange: (tab: 'chats' | 'groups' | 'calls') => void;
}

const TabBar: React.FC<TabBarProps> = ({ activeTab, onTabChange }) => {
  return (
    <div className="tab-bar">
      <button 
        className={`tab-item ${activeTab === 'chats' ? 'active' : ''}`}
        onClick={() => onTabChange('chats')}
      >
        <span className="tab-icon">💬</span>
        <span className="tab-label">Chats</span>
      </button>
      
      <button 
        className={`tab-item ${activeTab === 'groups' ? 'active' : ''}`}
        onClick={() => onTabChange('groups')}
      >
        <span className="tab-icon">👥</span>
        <span className="tab-label">Groups</span>
      </button>
      
      <button 
        className={`tab-item ${activeTab === 'calls' ? 'active' : ''}`}
        onClick={() => onTabChange('calls')}
      >
        <span className="tab-icon">📞</span>
        <span className="tab-label">Calls</span>
      </button>
    </div>
  );
};

export default TabBar;