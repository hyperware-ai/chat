import React, { useMemo, useState } from 'react';
import { GroupMessage as GroupMessageType } from '../../types/groups';
import GroupMessageMenu from './GroupMessageMenu';
import './GroupMessage.css';

interface ChildThreadInfo {
  id: string;
  title: string | null;
}

interface GroupMessageProps {
  message: GroupMessageType;
  currentNode?: string | null;
  onStartThread?: (parentThreadId: string, rootMessageId?: string) => void | Promise<void>;
  onOpenThread?: (threadId: string) => void;
  isActiveThread?: boolean;
  onReply?: (messageId: string) => void;
  onSetEditingMessage?: (message: { id: string; content: string }) => void;
  onDelete?: (messageId: string) => void;
  onReact?: (messageId: string, emoji: string) => void;
  childThread?: ChildThreadInfo | null;
  allMessages?: GroupMessageType[];
}

const formatTime = (timestamp: number) => {
  const date = new Date(timestamp * 1000);
  return date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
};

const GroupMessage = React.forwardRef<HTMLDivElement, GroupMessageProps>(
  ({ message, currentNode, onStartThread, onOpenThread, isActiveThread, onReply, onSetEditingMessage, onDelete, onReact, childThread, allMessages }, ref) => {
    const isMine = currentNode && message.sender === currentNode;
    const statusLabel =
      message.status === 'sending'
        ? 'Sending…'
        : message.status === 'failed'
        ? 'Failed'
        : '';
    const [menuPosition, setMenuPosition] = useState<{ x: number; y: number } | null>(null);

    const handleContextMenu = (e: React.MouseEvent) => {
      e.preventDefault();
      setMenuPosition({ x: e.clientX, y: e.clientY });
    };

    const groupedReactions = useMemo(() => {
      const grouped: Record<string, string[]> = {};
      message.reactions?.forEach((reaction) => {
        if (!grouped[reaction.emoji]) {
          grouped[reaction.emoji] = [];
        }
        grouped[reaction.emoji].push(reaction.user);
      });
      return grouped;
    }, [message.reactions]);

    const viewerId = currentNode ?? '';

    const handleReaction = (emoji: string) => {
      if (onReact) {
        onReact(message.id, emoji);
      }
    };

    const replyToMessage = message.replyTo && allMessages
      ? allMessages.find(m => m.id === message.replyTo)
      : null;

    const handleReplyClick = () => {
      if (message.replyTo) {
        const element = document.getElementById(`group-message-${message.replyTo}`);
        if (element) {
          element.scrollIntoView({ behavior: 'smooth', block: 'center' });
          element.classList.add('highlight');
          setTimeout(() => element.classList.remove('highlight'), 2000);
        }
      }
    };

    return (
      <>
        <div className={`group-message ${isMine ? 'mine' : ''}`} ref={ref} id={`group-message-${message.id}`}>
          {!isMine && <div className="group-message-sender">{message.sender}</div>}
          {replyToMessage && (
            <div className="group-message-reply-to" onClick={handleReplyClick}>
              <div className="group-message-reply-label">↩ Reply to {replyToMessage.sender}</div>
              <div className="group-message-reply-content">{replyToMessage.content}</div>
            </div>
          )}
          <div className="group-message-bubble" onContextMenu={handleContextMenu}>
            <div className="group-message-text">{message.content}</div>
            <div className="group-message-meta">
              <span>{formatTime(message.timestamp)}</span>
              {statusLabel && <span className="group-message-status">{statusLabel}</span>}
            </div>
            {message.reactions && message.reactions.length > 0 && (
              <div className="group-message-reactions">
                {Object.entries(groupedReactions).map(([emoji, users]) => (
                  <button
                    key={emoji}
                    className={`reaction ${
                      users.includes(viewerId) ? 'reacted' : ''
                    }`}
                    onClick={() => handleReaction(emoji)}
                    title={users.join(', ')}
                  >
                    {emoji} {users.length > 1 && users.length}
                  </button>
                ))}
              </div>
            )}
            {childThread && onOpenThread && (
              <button
                type="button"
                className="group-message-thread-indicator has-thread"
                onClick={(e) => {
                  e.preventDefault();
                  e.stopPropagation();
                  onOpenThread(childThread.id);
                }}
              >
                <span className="thread-icon">↳</span>
                <span className="thread-label">
                  {childThread.title || 'Thread'}
                </span>
              </button>
            )}
          </div>
        </div>
        {menuPosition && (
          <GroupMessageMenu
            message={message}
            position={menuPosition}
            onClose={() => setMenuPosition(null)}
            onStartThread={onStartThread}
            onOpenThread={onOpenThread}
            onReply={onReply}
            onSetEditingMessage={onSetEditingMessage}
            onDelete={onDelete}
            onReact={onReact}
            currentNode={currentNode}
            childThread={childThread}
          />
        )}
      </>
    );
  });

export default GroupMessage;
