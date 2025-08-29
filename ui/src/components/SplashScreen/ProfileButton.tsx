import React from 'react';
import { useChatStore } from '../../store/chat';
import './ProfileButton.css';

interface ProfileButtonProps {
  onClick: () => void;
}

const ProfileButton: React.FC<ProfileButtonProps> = ({ onClick }) => {
  const { profile } = useChatStore();
  
  const getInitial = () => {
    return profile?.name?.charAt(0).toUpperCase() || 'U';
  };

  return (
    <button className="profile-button" onClick={onClick}>
      {profile?.profilePic ? (
        <img src={profile.profilePic} alt={profile.name} className="profile-pic" />
      ) : (
        <div className="profile-initial">{getInitial()}</div>
      )}
    </button>
  );
};

export default ProfileButton;