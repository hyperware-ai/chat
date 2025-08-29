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
      stt_api_key: value || null,
    });
  };

  return (
    <div className="general-settings">
      <div className="setting-item">
        <label>
          <input
            type="checkbox"
            checked={settings.show_images}
            onChange={() => handleToggle('show_images')}
          />
          <span>Show images in chats</span>
        </label>
      </div>
      
      <div className="setting-item">
        <label>
          <input
            type="checkbox"
            checked={settings.show_profile_pics}
            onChange={() => handleToggle('show_profile_pics')}
          />
          <span>Show profile pictures</span>
        </label>
      </div>
      
      <div className="setting-item">
        <label>
          <input
            type="checkbox"
            checked={settings.combine_chats_groups}
            onChange={() => handleToggle('combine_chats_groups')}
          />
          <span>Combine Chats & Groups tabs</span>
        </label>
      </div>
      
      <div className="setting-item">
        <label>
          <input
            type="checkbox"
            checked={settings.allow_browser_chats}
            onChange={() => handleToggle('allow_browser_chats')}
          />
          <span>Allow browser chats</span>
        </label>
      </div>
      
      <div className="setting-item">
        <label>
          <input
            type="checkbox"
            checked={settings.stt_enabled}
            onChange={() => handleToggle('stt_enabled')}
          />
          <span>Enable Speech-to-Text for voice notes</span>
        </label>
      </div>
      
      {settings.stt_enabled && (
        <div className="setting-item">
          <label>STT API Key:</label>
          <input
            type="text"
            value={settings.stt_api_key || ''}
            onChange={(e) => handleSTTKeyChange(e.target.value)}
            placeholder="Enter your STT API key"
          />
        </div>
      )}
    </div>
  );
};

export default GeneralSettings;