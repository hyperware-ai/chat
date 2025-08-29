import React from 'react';
import { Chat } from '../../types/chat';
import { useChatStore } from '../../store/chat';
import Avatar from '../Common/Avatar';
import './ChatListItem.css';

interface ChatListItemProps {
  chat: Chat;
}

const ChatListItem: React.FC<ChatListItemProps> = ({ chat }) => {
  const { setActiveChat } = useChatStore();
  
  const getLastMessage = () => {
    if (chat.messages.length === 0) return 'No messages yet';
    const lastMsg = chat.messages[chat.messages.length - 1];
    return lastMsg.content;
  };
  
  const formatTime = (timestamp: number) => {
    const date = new Date(timestamp * 1000);
    const now = new Date();
    const diff = now.getTime() - date.getTime();
    const days = Math.floor(diff / (1000 * 60 * 60 * 24));
    
    if (days === 0) {
      return date.toLocaleTimeString('en-US', { hour: '2-digit', minute: '2-digit' });
    } else if (days === 1) {
      return 'Yesterday';
    } else if (days < 7) {
      return date.toLocaleDateString('en-US', { weekday: 'short' });
    } else {
      return date.toLocaleDateString('en-US', { month: 'short', day: 'numeric' });
    }
  };

  return (
    <div className="chat-list-item" onClick={() => setActiveChat(chat)}>
      <Avatar name={chat.counterparty} />
      
      <div className="chat-info">
        <div className="chat-header">
          <span className="chat-name">{chat.counterparty}</span>
          <span className="chat-time">{formatTime(chat.last_activity)}</span>
        </div>
        <div className="chat-preview">
          <span className="last-message">{getLastMessage()}</span>
          {chat.unread_count > 0 && (
            <span className="unread-badge">{chat.unread_count}</span>
          )}
        </div>
      </div>
    </div>
  );
};

export default ChatListItem;