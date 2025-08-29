import React, { useState, useMemo } from 'react';
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
  const { activeChat, loadChats, settings } = useChatStore();

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

  // Parse message content for links and images
  const renderMessageContent = useMemo(() => {
    const urlRegex = /(https?:\/\/[^\s]+)/g;
    const imageRegex = /\.(jpg|jpeg|png|gif|webp|svg|bmp)$/i;
    
    const parts = message.content.split(urlRegex);
    
    return parts.map((part, index) => {
      // Check if this part is a URL
      if (part.match(urlRegex)) {
        // Check if it's an image URL
        if (imageRegex.test(part) && settings?.showImages) {
          return (
            <div key={index} style={{ margin: '8px 0' }}>
              <a href={part} target="_blank" rel="noopener noreferrer">
                <img 
                  src={part} 
                  alt="Image" 
                  style={{ 
                    maxWidth: '100%', 
                    maxHeight: '300px',
                    borderRadius: '8px',
                    display: 'block'
                  }}
                  onError={(e) => {
                    // If image fails to load, show as link instead
                    const target = e.target as HTMLImageElement;
                    target.style.display = 'none';
                    const link = document.createElement('a');
                    link.href = part;
                    link.target = '_blank';
                    link.rel = 'noopener noreferrer';
                    link.textContent = part;
                    link.style.color = isOwn ? '#ffffff' : '#4da6ff';
                    link.style.textDecoration = 'underline';
                    target.parentNode?.replaceChild(link, target);
                  }}
                />
              </a>
            </div>
          );
        }
        // Regular link
        return (
          <a 
            key={index}
            href={part} 
            target="_blank" 
            rel="noopener noreferrer"
            style={{ 
              color: isOwn ? '#ffffff' : '#4da6ff',  // Brighter blue for better visibility
              textDecoration: 'underline'
            }}
          >
            {part}
          </a>
        );
      }
      // Regular text
      return <span key={index}>{part}</span>;
    });
  }, [message.content, settings?.showImages, isOwn]);

  return (
    <>
      <div 
        id={`message-${message.id}`}
        className={`message ${isOwn ? 'own' : 'other'}`}
        onContextMenu={handleLongPress}
      >
        {message.replyTo && (
          <div 
            className="reply-to"
            onClick={() => {
              // Scroll to the original message
              const element = document.getElementById(`message-${message.replyTo}`);
              if (element) {
                element.scrollIntoView({ behavior: 'smooth', block: 'center' });
                // Add a highlight animation
                element.classList.add('highlight');
                setTimeout(() => element.classList.remove('highlight'), 2000);
              }
            }}
            style={{ cursor: 'pointer' }}
          >
            <div className="reply-to-label">↩ Reply</div>
            <div className="reply-to-content">
              {activeChat?.messages.find(m => m.id === message.replyTo)?.content || 'Message not found'}
            </div>
          </div>
        )}
        
        <div className="message-content">
          {/* If this is a file/image message with file info, show it specially */}
          {message.fileInfo && message.messageType === 'Image' && settings?.showImages ? (
            <div>
              <img 
                src={message.fileInfo.url} 
                alt={message.fileInfo.filename}
                style={{ 
                  maxWidth: '100%', 
                  maxHeight: '300px',
                  borderRadius: '8px',
                  display: 'block',
                  marginBottom: '8px'
                }}
              />
              <div style={{ fontSize: '12px', opacity: 0.8 }}>{message.fileInfo.filename}</div>
            </div>
          ) : message.fileInfo && message.messageType === 'File' ? (
            <div>
              <div style={{ marginBottom: '8px' }}>📎 {message.fileInfo.filename}</div>
              <div style={{ fontSize: '12px', opacity: 0.8 }}>
                {(message.fileInfo.size / 1024).toFixed(1)} KB
              </div>
            </div>
          ) : (
            renderMessageContent
          )}
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