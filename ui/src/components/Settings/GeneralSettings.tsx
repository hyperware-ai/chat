import React from 'react';
import { useChatStore } from '../../store/chat';
import './GeneralSettings.css';

const GeneralSettings: React.FC = () => {
  const { settings, updateSettings } = useChatStore();

  const handleToggle = (key: keyof typeof settings) => {
    updateSettings({
      ...settings,
      [key]: !settings[key],
    });
  };

  const handleSTTKeyChange = (value: string) => {
    updateSettings({
      ...settings,
      sttApiKey: value || null,
    });
  };

  return (
    <div className="general-settings">
      <div className="setting-item">
        <label>
          <input
            type="checkbox"
            checked={settings.showImages}
            onChange={() => handleToggle('showImages')}
          />
          <span>Show images in chats</span>
        </label>
      </div>
      
      <div className="setting-item">
        <label>
          <input
            type="checkbox"
            checked={settings.showProfilePics}
            onChange={() => handleToggle('showProfilePics')}
          />
          <span>Show profile pictures</span>
        </label>
      </div>
      
      <div className="setting-item">
        <label>
          <input
            type="checkbox"
            checked={settings.combineChatsGroups}
            onChange={() => handleToggle('combineChatsGroups')}
          />
          <span>Combine Chats & Groups tabs</span>
        </label>
      </div>
      
      <div className="setting-item">
        <label>
          <input
            type="checkbox"
            checked={settings.allowBrowserChats}
            onChange={() => handleToggle('allowBrowserChats')}
          />
          <span>Allow browser chats</span>
        </label>
      </div>
      
      <div className="setting-item">
        <label>
          <input
            type="checkbox"
            checked={settings.sttEnabled}
            onChange={() => handleToggle('sttEnabled')}
          />
          <span>Enable Speech-to-Text for voice notes</span>
        </label>
      </div>
      
      {settings.sttEnabled && (
        <div className="setting-item">
          <label>STT API Key:</label>
          <input
            type="text"
            value={settings.sttApiKey || ''}
            onChange={(e) => handleSTTKeyChange(e.target.value)}
            placeholder="Enter your STT API key"
          />
        </div>
      )}
    </div>
  );
};

export default GeneralSettings;