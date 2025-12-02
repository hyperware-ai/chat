import React from 'react';
import { GroupMessage as GroupMessageType } from '../../types/groups';
import './GroupMessage.css';

interface GroupMessageProps {
  message: GroupMessageType;
  currentNode?: string | null;
}

const formatTime = (timestamp: number) => {
  const date = new Date(timestamp * 1000);
  return date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
};

const GroupMessage: React.FC<GroupMessageProps> = ({ message, currentNode }) => {
  const isMine = currentNode && message.sender === currentNode;
  const statusLabel =
    message.status === 'sending'
      ? 'Sending…'
      : message.status === 'failed'
      ? 'Failed'
      : '';

  return (
    <div className={`group-message ${isMine ? 'mine' : ''}`}>
      {!isMine && <div className="group-message-sender">{message.sender}</div>}
      <div className="group-message-bubble">
        <div className="group-message-text">{message.content}</div>
        <div className="group-message-meta">
          <span>{formatTime(message.timestamp)}</span>
          {statusLabel && <span className="group-message-status">{statusLabel}</span>}
        </div>
      </div>
    </div>
  );
};

export default GroupMessage;
