import React, { useState } from 'react';
import { Chat } from '#caller-utils';
import { useChatStore } from '../../store/chat';
import Avatar from '../Common/Avatar';
import ChatSettings from './ChatSettings';
import './ChatHeader.css';

interface ChatHeaderProps {
  chat: Chat.Chat;
}

const ChatHeader: React.FC<ChatHeaderProps> = ({ chat }) => {
  const { setActiveChat } = useChatStore();
  const [showSettings, setShowSettings] = useState(false);

  return (
    <>
      <div className="chat-header">
        <button className="back-button" onClick={() => setActiveChat(null)}>
          ←
        </button>
        
        <div className="chat-header-info" onClick={() => setShowSettings(true)}>
          <Avatar 
            name={chat.counterparty} 
            profilePic={chat.counterparty_profile?.profile_pic}
            size="small" 
          />
          <span className="chat-header-name">{chat.counterparty}</span>
        </div>
        
        <button className="voice-call-button" aria-label="Start voice call">
          📞
        </button>
      </div>
      
      {showSettings && (
        <ChatSettings chat={chat} onClose={() => setShowSettings(false)} />
      )}
    </>
  );
};

export default ChatHeader;