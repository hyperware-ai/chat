import React, { useEffect, useRef, useState } from 'react';
import { useChatStore } from '../../store/chat';
import MessageList from './MessageList';
import MessageInput from './MessageInput';
import ChatHeader from './ChatHeader';
import './ChatView.css';

const ChatView: React.FC = () => {
  const { activeChat, markChatAsRead, setActiveChat } = useChatStore();
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const messagesContainerRef = useRef<HTMLDivElement>(null);
  const [swipeX, setSwipeX] = useState(0);
  const [isSwiping, setIsSwiping] = useState(false);
  const [showOfflineTooltip, setShowOfflineTooltip] = useState(false);
  const [showScrollButton, setShowScrollButton] = useState(false);
  const startXRef = useRef(0);
  const startYRef = useRef(0);
  const chatViewRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (activeChat) {
      markChatAsRead(activeChat.id);

      // Check if this is a new chat with no messages or only "Sent" messages
      // This indicates the node might be offline
      const hasOnlySentMessages = activeChat.messages.length > 0 &&
        activeChat.messages.every(msg =>
          msg.sender !== activeChat.counterparty &&
          (msg.status === 'Sent' || msg.status === 'Sending')
        );

      const isNewChat = activeChat.messages.length === 0 ||
        (activeChat.messages.length === 1 && activeChat.messages[0].sender === 'System');

      if (isNewChat || hasOnlySentMessages) {
        setShowOfflineTooltip(true);
        // Auto-hide after 10 seconds
        const timer = setTimeout(() => setShowOfflineTooltip(false), 10000);
        return () => clearTimeout(timer);
      }
    }
  }, [activeChat, markChatAsRead]);

  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [activeChat?.messages]);

  // Check if scrolled to bottom
  useEffect(() => {
    const container = messagesContainerRef.current;
    if (!container) return;

    const checkScroll = () => {
      const { scrollTop, scrollHeight, clientHeight } = container;
      // Consider "at bottom" if within 100px of bottom
      const isAtBottom = scrollHeight - scrollTop - clientHeight < 100;
      setShowScrollButton(!isAtBottom);
    };

    container.addEventListener('scroll', checkScroll);
    checkScroll(); // Check initial state

    return () => container.removeEventListener('scroll', checkScroll);
  }, [activeChat]);

  // Hide tooltip when user sends a message or taps
  const handleUserInteraction = () => {
    setShowOfflineTooltip(false);
  };

  // Scroll to bottom function
  const scrollToBottom = () => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  };

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
      onClick={handleUserInteraction}
      style={{
        transform: `translateX(${swipeX}px)`,
        transition: isSwiping ? 'none' : 'transform 0.3s ease-out',
        opacity: isSwiping ? 1 - (swipeX / (window.innerWidth * 1.5)) : 1
      }}
    >
      <ChatHeader chat={activeChat} />

      {showOfflineTooltip && (
        <div className="offline-tooltip" onClick={handleUserInteraction}>
          <div className="offline-tooltip-content" style={{ textAlign: 'center' }}>
            Users can be messaged whether they are online or not. Messages display their delivery status:
            <br />
            • ✓ Attempting to deliver message
            <br />
            • ✓✓ Message successfully delivered
          </div>
        </div>
      )}

      <div className="messages-container" ref={messagesContainerRef}>
        <MessageList messages={activeChat.messages} />
        <div ref={messagesEndRef} />
      </div>

      {showScrollButton && (
        <button 
          className="scroll-to-bottom-button"
          onClick={scrollToBottom}
          aria-label="Scroll to bottom"
        >
          ↓
        </button>
      )}

      <MessageInput chatId={activeChat.id} onSendMessage={handleUserInteraction} />
    </div>
  );
};

export default ChatView;
