import React from 'react';
import { ChatMessage } from '../../types/chat';
import Message from './Message';
import './MessageList.css';

interface MessageListProps {
  messages: ChatMessage[];
}

const MessageList: React.FC<MessageListProps> = ({ messages }) => {
  const formatDate = (timestamp: number) => {
    const date = new Date(timestamp * 1000);
    const today = new Date();
    const yesterday = new Date(today);
    yesterday.setDate(yesterday.getDate() - 1);
    
    if (date.toDateString() === today.toDateString()) {
      return 'Today';
    } else if (date.toDateString() === yesterday.toDateString()) {
      return 'Yesterday';
    } else {
      return date.toLocaleDateString('en-US', { 
        weekday: 'long', 
        month: 'long', 
        day: 'numeric' 
      });
    }
  };

  let lastDate = '';

  return (
    <div className="message-list">
      {messages.map((message) => {
        const messageDate = formatDate(message.timestamp);
        const showDate = messageDate !== lastDate;
        lastDate = messageDate;
        
        return (
          <React.Fragment key={message.id}>
            {showDate && (
              <div className="date-separator">
                <span>{messageDate}</span>
              </div>
            )}
            <Message 
              message={message} 
              isOwn={message.sender === (window as any).our?.node}
            />
          </React.Fragment>
        );
      })}
    </div>
  );
};

export default MessageList;