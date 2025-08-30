import React, { useEffect, useRef, useState } from 'react';
import { useChatStore } from '../../store/chat';
import MessageList from './MessageList';
import MessageInput from './MessageInput';
import ChatHeader from './ChatHeader';
import './ChatView.css';

const ChatView: React.FC = () => {
  const { activeChat, markChatAsRead, setActiveChat } = useChatStore();
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const [swipeX, setSwipeX] = useState(0);
  const [isSwiping, setIsSwiping] = useState(false);
  const startXRef = useRef(0);
  const startYRef = useRef(0);
  const chatViewRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (activeChat) {
      markChatAsRead(activeChat.id);
    }
  }, [activeChat, markChatAsRead]);

  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [activeChat?.messages]);

  // Swipe handlers for navigation
  const handleTouchStart = (e: React.TouchEvent) => {
    // Only handle swipe if starting from left edge
    if (e.touches[0].clientX < 30) {
      startXRef.current = e.touches[0].clientX;
      startYRef.current = e.touches[0].clientY;
      setIsSwiping(false);
    }
  };

  const handleTouchMove = (e: React.TouchEvent) => {
    if (startXRef.current === 0) return;
    
    const currentX = e.touches[0].clientX;
    const currentY = e.touches[0].clientY;
    const deltaX = currentX - startXRef.current;
    const deltaY = Math.abs(currentY - startYRef.current);
    
    // Only trigger swipe if horizontal movement is greater than vertical
    // and swiping right from left edge
    if (deltaX > 10 && deltaY < 50 && startXRef.current < 30) {
      setIsSwiping(true);
      // Limit swipe distance
      const limitedDeltaX = Math.min(deltaX, window.innerWidth * 0.8);
      setSwipeX(limitedDeltaX);
      
      // Add haptic feedback when reaching threshold
      if (limitedDeltaX >= window.innerWidth * 0.3 && 'vibrate' in navigator) {
        navigator.vibrate(10);
      }
    }
  };

  const handleTouchEnd = () => {
    if (swipeX >= window.innerWidth * 0.3) {
      // Go back to chat list
      setActiveChat(null);
    }
    setSwipeX(0);
    setIsSwiping(false);
    startXRef.current = 0;
  };

  if (!activeChat) {
    return null;
  }

  return (
    <div 
      ref={chatViewRef}
      className={`chat-view ${isSwiping ? 'swiping' : ''}`}
      onTouchStart={handleTouchStart}
      onTouchMove={handleTouchMove}
      onTouchEnd={handleTouchEnd}
      style={{
        transform: `translateX(${swipeX}px)`,
        transition: isSwiping ? 'none' : 'transform 0.3s ease-out',
        opacity: isSwiping ? 1 - (swipeX / (window.innerWidth * 1.5)) : 1
      }}
    >
      <ChatHeader chat={activeChat} />
      
      <div className="messages-container">
        <MessageList messages={activeChat.messages} />
        <div ref={messagesEndRef} />
      </div>
      
      <MessageInput chatId={activeChat.id} />
    </div>
  );
};

export default ChatView;