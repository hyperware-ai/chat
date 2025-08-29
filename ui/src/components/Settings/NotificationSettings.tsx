import React from 'react';
import { useChatStore } from '../../store/chat';
import './NotificationSettings.css';

const NotificationSettings: React.FC = () => {
  const { settings, updateSettings } = useChatStore();

  const handleToggle = (key: keyof typeof settings) => {
    updateSettings({
      ...settings,
      [key]: !settings[key],
    });
  };

  return (
    <div className="notification-settings">
      <div className="setting-item">
        <label>
          <input
            type="checkbox"
            checked={settings.notifyChats}
            onChange={() => handleToggle('notifyChats')}
          />
          <span>Notify for new chat messages</span>
        </label>
      </div>
      
      <div className="setting-item">
        <label>
          <input
            type="checkbox"
            checked={settings.notifyGroups}
            onChange={() => handleToggle('notifyGroups')}
          />
          <span>Notify for group messages</span>
        </label>
      </div>
      
      <div className="setting-item">
        <label>
          <input
            type="checkbox"
            checked={settings.notifyCalls}
            onChange={() => handleToggle('notifyCalls')}
          />
          <span>Notify for incoming calls</span>
        </label>
      </div>
    </div>
  );
};

export default NotificationSettings;