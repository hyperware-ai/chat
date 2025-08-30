import React, { useState, useMemo, useRef, useEffect } from 'react';
import { ChatMessage } from '../../types/chat';
import MessageMenu from './MessageMenu';
import './Message.css';
import { add_reaction, remove_reaction } from '../../../../target/ui/caller-utils';
import { useChatStore } from '../../store/chat';

interface MessageProps {
  message: ChatMessage;
  isOwn: boolean;
}

const Message: React.FC<MessageProps> = ({ message, isOwn }) => {
  const [showMenu, setShowMenu] = useState(false);
  const [menuPosition, setMenuPosition] = useState({ x: 0, y: 0 });
  const [swipeX, setSwipeX] = useState(0);
  const [isSwiping, setIsSwiping] = useState(false);
  const { activeChat, loadChats, settings, setReplyingTo } = useChatStore();
  const messageRef = useRef<HTMLDivElement>(null);
  const startXRef = useRef(0);
  const startYRef = useRef(0);

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

  // Swipe handlers for swipe-to-reply
  const handleTouchStart = (e: React.TouchEvent) => {
    startXRef.current = e.touches[0].clientX;
    startYRef.current = e.touches[0].clientY;
    setIsSwiping(false);
  };

  const handleTouchMove = (e: React.TouchEvent) => {
    const currentX = e.touches[0].clientX;
    const currentY = e.touches[0].clientY;
    const deltaX = currentX - startXRef.current;
    const deltaY = Math.abs(currentY - startYRef.current);
    
    // Only trigger swipe if horizontal movement is greater than vertical
    if (Math.abs(deltaX) > 10 && deltaY < 50) {
      setIsSwiping(true);
      // Limit swipe distance
      const limitedDeltaX = Math.min(Math.max(deltaX, -80), 80);
      setSwipeX(limitedDeltaX);
      
      // Add haptic feedback when reaching threshold
      if (Math.abs(limitedDeltaX) >= 60 && 'vibrate' in navigator) {
        navigator.vibrate(10);
      }
    }
  };

  const handleTouchEnd = () => {
    if (Math.abs(swipeX) >= 60) {
      // Trigger reply action
      setReplyingTo(message);
      // Add visual feedback
      if (messageRef.current) {
        messageRef.current.classList.add('reply-triggered');
        setTimeout(() => {
          messageRef.current?.classList.remove('reply-triggered');
        }, 300);
      }
    }
    setSwipeX(0);
    setIsSwiping(false);
  };

  const handleReaction = async (emoji: string) => {
    try {
      // Check if user already reacted with this emoji
      const existingReaction = message.reactions?.find(
        r => r.user === window.our?.node && r.emoji === emoji
      );
      
      if (existingReaction) {
        await remove_reaction(JSON.stringify({ 
          message_id: message.id, 
          emoji 
        }));
      } else {
        await add_reaction(JSON.stringify({ 
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
        if (imageRegex.test(part) && settings?.show_images) {
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
  }, [message.content, settings?.show_images, isOwn]);

  return (
    <>
      <div 
        ref={messageRef}
        id={`message-${message.id}`}
        className={`message ${isOwn ? 'own' : 'other'} ${isSwiping ? 'swiping' : ''}`}
        onContextMenu={handleLongPress}
        onTouchStart={handleTouchStart}
        onTouchMove={handleTouchMove}
        onTouchEnd={handleTouchEnd}
        style={{
          transform: `translateX(${swipeX}px)`,
          transition: isSwiping ? 'none' : 'transform 0.2s ease-out'
        }}
      >
        {message.reply_to && (
          <div 
            className="reply-to"
            onClick={() => {
              // Scroll to the original message
              const element = document.getElementById(`message-${message.reply_to}`);
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
              {activeChat?.messages.find(m => m.id === message.reply_to)?.content || 'Message not found'}
            </div>
          </div>
        )}
        
        <div className="message-content">
          {/* If this is a file/image message with file info, show it specially */}
          {message.file_info && message.message_type === 'Image' && settings?.show_images ? (
            <div>
              <img 
                src={message.file_info.url} 
                alt={message.file_info.filename}
                style={{ 
                  maxWidth: '100%', 
                  maxHeight: '300px',
                  borderRadius: '8px',
                  display: 'block',
                  marginBottom: '8px'
                }}
              />
              <div style={{ fontSize: '12px', opacity: 0.8 }}>{message.file_info.filename}</div>
            </div>
          ) : message.file_info && message.message_type === 'File' ? (
            <div>
              <div style={{ marginBottom: '8px' }}>📎 {message.file_info.filename}</div>
              <div style={{ fontSize: '12px', opacity: 0.8 }}>
                {(message.file_info.size / 1024).toFixed(1)} KB
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