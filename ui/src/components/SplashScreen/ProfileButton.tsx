import React, { useEffect } from 'react';
import { useChatStore } from '../../store/chat';
import Avatar from '../Common/Avatar';
import './ProfileButton.css';

interface ProfileButtonProps {
  onClick: () => void;
}

const ProfileButton: React.FC<ProfileButtonProps> = ({ onClick }) => {
  const { profile, loadProfile } = useChatStore();
  
  // Ensure profile is loaded
  useEffect(() => {
    // Always try to load profile on mount
    loadProfile();
  }, [loadProfile]);

  return (
    <button className="profile-button" onClick={onClick}>
      <Avatar 
        name={profile?.name || 'User'}
        profilePic={profile?.profile_pic}
        size="medium"
      />
    </button>
  );
};

export default ProfileButton;