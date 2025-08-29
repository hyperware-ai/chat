import React, { useState } from 'react';
import { ChatMessage } from '../../types/chat';
import MessageMenu from './MessageMenu';
import './Message.css';
import { addReaction, removeReaction } from '../../../../target/ui/caller-utils';
import { useChatStore } from '../../store/chat';

interface MessageProps {
  message: ChatMessage;
  isOwn: boolean;
}

const Message: React.FC<MessageProps> = ({ message, isOwn }) => {
  const [showMenu, setShowMenu] = useState(false);
  const [menuPosition, setMenuPosition] = useState({ x: 0, y: 0 });
  const { activeChat, loadChats } = useChatStore();

  const formatTime = (timestamp: number) => {
    const date = new Date(timestamp * 1000);
    return date.toLocaleTimeString('en-US', { 
      hour: '2-digit', 
      minute: '2-digit' 
    });
  };

  const getStatusIcon = () => {
    switch (message.status) {
      case 'Sending':
        return '...';
      case 'Sent':
        return '✓';
      case 'Delivered':
        return '✓✓';
      case 'Failed':
        return '❌';
      default:
        return '';
    }
  };

  const handleLongPress = (e: React.MouseEvent | React.TouchEvent) => {
    e.preventDefault();
    const rect = (e.target as HTMLElement).getBoundingClientRect();
    setMenuPosition({ x: rect.left, y: rect.top });
    setShowMenu(true);
  };

  const handleReaction = async (emoji: string) => {
    try {
      // Check if user already reacted with this emoji
      const existingReaction = message.reactions?.find(
        r => r.user === window.our?.node && r.emoji === emoji
      );
      
      if (existingReaction) {
        await removeReaction(JSON.stringify({ 
          message_id: message.id, 
          emoji 
        }));
      } else {
        await addReaction(JSON.stringify({ 
          message_id: message.id, 
          emoji 
        }));
      }
      
      // Reload chats to get updated reactions
      await loadChats();
    } catch (err) {
      console.error('Error handling reaction:', err);
    }
  };

  const groupReactions = () => {
    const grouped: { [emoji: string]: string[] } = {};
    message.reactions?.forEach(reaction => {
      if (!grouped[reaction.emoji]) {
        grouped[reaction.emoji] = [];
      }
      grouped[reaction.emoji].push(reaction.user);
    });
    return grouped;
  };

  return (
    <>
      <div 
        className={`message ${isOwn ? 'own' : 'other'}`}
        onContextMenu={handleLongPress}
      >
        {message.replyTo && (
          <div className="reply-to">
            <div className="reply-to-label">↩ Reply</div>
            <div className="reply-to-content">
              {activeChat?.messages.find(m => m.id === message.replyTo)?.content || 'Message not found'}
            </div>
          </div>
        )}
        
        <div className="message-content">
          {message.content}
        </div>
        
        <div className="message-footer">
          <span className="message-time">{formatTime(message.timestamp)}</span>
          {isOwn && (
            <span className="message-status">{getStatusIcon()}</span>
          )}
        </div>
        
        {message.reactions && message.reactions.length > 0 && (
          <div className="message-reactions">
            {Object.entries(groupReactions()).map(([emoji, users]) => (
              <button
                key={emoji}
                className={`reaction ${users.includes(window.our?.node || '') ? 'reacted' : ''}`}
                onClick={() => handleReaction(emoji)}
                title={users.join(', ')}
              >
                {emoji} {users.length > 1 && users.length}
              </button>
            ))}
          </div>
        )}
      </div>
      
      {showMenu && (
        <MessageMenu 
          message={message}
          isOwn={isOwn}
          position={menuPosition}
          onClose={() => setShowMenu(false)}
        />
      )}
    </>
  );
};

export default Message;