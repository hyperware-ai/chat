import React from 'react';
import { GroupMessage as GroupMessageType } from '../../types/groups';
import './GroupMessage.css';

interface GroupMessageProps {
  message: GroupMessageType;
  currentNode?: string | null;
  onStartThread?: () => void;
  onOpenThread?: (threadId: string) => void;
  isActiveThread?: boolean;
}

const formatTime = (timestamp: number) => {
  const date = new Date(timestamp * 1000);
  return date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
};

const GroupMessage: React.FC<GroupMessageProps> = ({
  message,
  currentNode,
  onStartThread,
  onOpenThread,
  isActiveThread,
}) => {
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
        {(onStartThread || onOpenThread) && (
          <div className="group-message-actions">
            {onOpenThread && (
              <button
                className="link"
                onClick={() => onOpenThread(message.threadId)}
                disabled={isActiveThread}
              >
                {isActiveThread ? 'In thread' : 'Open thread'}
              </button>
            )}
            {onStartThread && (
              <button className="link" onClick={onStartThread}>
                Start sub-thread
              </button>
            )}
          </div>
        )}
      </div>
    </div>
  );
};

export default GroupMessage;
