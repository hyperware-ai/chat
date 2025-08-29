import React, { useState } from 'react';
import { useChatStore } from '../../store/chat';
import Avatar from '../Common/Avatar';
import './ProfileSettings.css';

const ProfileSettings: React.FC = () => {
  const { profile, updateProfile } = useChatStore();
  const [name, setName] = useState(profile?.name || '');
  const [profilePic, setProfilePic] = useState(profile?.profilePic || '');

  const handleSave = async () => {
    await updateProfile({
      name,
      profilePic: profilePic || null,
    });
  };

  return (
    <div className="profile-settings">
      <div className="profile-preview">
        <Avatar name={name} profilePic={profilePic || null} size="large" />
      </div>
      
      <div className="form-group">
        <label htmlFor="name">Display Name</label>
        <input
          type="text"
          id="name"
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder="Enter your name"
        />
      </div>
      
      <div className="form-group">
        <label htmlFor="profilePic">Profile Picture URL</label>
        <input
          type="text"
          id="profilePic"
          value={profilePic}
          onChange={(e) => setProfilePic(e.target.value)}
          placeholder="Enter image URL (optional)"
        />
      </div>
      
      <button className="save-button" onClick={handleSave}>
        Save Profile
      </button>
    </div>
  );
};

export default ProfileSettings;