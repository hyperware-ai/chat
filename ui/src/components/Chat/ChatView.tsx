import React, { useEffect, useRef } from 'react';
import { useChatStore } from '../../store/chat';
import MessageList from './MessageList';
import MessageInput from './MessageInput';
import ChatHeader from './ChatHeader';
import './ChatView.css';

const ChatView: React.FC = () => {
  const { activeChat, markChatAsRead } = useChatStore();
  const messagesEndRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (activeChat) {
      markChatAsRead(activeChat.id);
    }
  }, [activeChat, markChatAsRead]);

  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [activeChat?.messages]);

  if (!activeChat) {
    return null;
  }

  return (
    <div className="chat-view">
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