import React from 'react';
import { GroupMessage as GroupMessageType } from '../../types/groups';
import './GroupMessage.css';

interface GroupMessageProps {
  message: GroupMessageType;
  currentNode?: string | null;
  onStartThread?: (parentThreadId: string) => void | Promise<void>;
  onOpenThread?: (threadId: string) => void;
  isActiveThread?: boolean;
  onJumpToParent?: (parentId: string) => void;
}

const formatTime = (timestamp: number) => {
  const date = new Date(timestamp * 1000);
  return date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
};

const GroupMessage = React.forwardRef<HTMLDivElement, GroupMessageProps>(
  ({ message, currentNode, onStartThread, onOpenThread, isActiveThread, onJumpToParent }, ref) => {
  const isMine = currentNode && message.sender === currentNode;
  const statusLabel =
    message.status === 'sending'
      ? 'Sending…'
      : message.status === 'failed'
      ? 'Failed'
      : '';

  return (
    <div className={`group-message ${isMine ? 'mine' : ''}`} ref={ref}>
      {!isMine && <div className="group-message-sender">{message.sender}</div>}
      <div className="group-message-bubble">
        <div className="group-message-text">{message.content}</div>
        <div className="group-message-meta">
          <span>{formatTime(message.timestamp)}</span>
          {statusLabel && <span className="group-message-status">{statusLabel}</span>}
        </div>
        {(onStartThread || onOpenThread) && (
          <div className="group-message-actions">
          {onJumpToParent && message.replyTo && (
            <button className="link" onClick={() => onJumpToParent(message.replyTo!)}>
              Jump to parent
            </button>
          )}
            <button
              className="link"
              onClick={() => {
                if (navigator.clipboard) {
                  navigator.clipboard.writeText(message.content).catch(() => {});
                }
              }}
            >
              Copy
            </button>
            <button className="link" disabled title="Reactions require backend support">
              React (disabled)
            </button>
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
              <button className="link" onClick={() => onStartThread(message.threadId)}>
                Start sub-thread
              </button>
            )}
          </div>
        )}
      </div>
    </div>
  );
});

export default GroupMessage;
