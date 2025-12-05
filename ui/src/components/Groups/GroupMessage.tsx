import React, { useMemo, useState } from 'react';
import { GroupMessage as GroupMessageType } from '../../types/groups';
import GroupMessageMenu from './GroupMessageMenu';
import './GroupMessage.css';

interface GroupMessageProps {
  message: GroupMessageType;
  currentNode?: string | null;
  onStartThread?: (parentThreadId: string, rootMessageId?: string) => void | Promise<void>;
  onOpenThread?: (threadId: string) => void;
  isActiveThread?: boolean;
  onJumpToParent?: (parentId: string) => void;
  onReply?: (messageId: string) => void;
  onEdit?: (messageId: string, content: string) => void;
  onDelete?: (messageId: string) => void;
  onForward?: (messageId: string) => void;
  onReact?: (messageId: string, emoji: string) => void;
}

const formatTime = (timestamp: number) => {
  const date = new Date(timestamp * 1000);
  return date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
};

const GroupMessage = React.forwardRef<HTMLDivElement, GroupMessageProps>(
  ({ message, currentNode, onStartThread, onOpenThread, isActiveThread, onJumpToParent, onReply, onEdit, onDelete, onForward, onReact }, ref) => {
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

    return (
      <>
        <div className={`group-message ${isMine ? 'mine' : ''}`} ref={ref}>
          {!isMine && <div className="group-message-sender">{message.sender}</div>}
          <div className="group-message-bubble" onContextMenu={handleContextMenu}>
            <div className="group-message-text">{message.content}</div>
            <div className="group-message-meta">
              <span>{formatTime(message.timestamp)}</span>
              {statusLabel && <span className="group-message-status">{statusLabel}</span>}
            </div>
            {(onOpenThread || (onJumpToParent && message.replyTo)) && (
              <div className="group-message-actions">
                {onJumpToParent && message.replyTo && (
                  <button className="link" onClick={() => onJumpToParent(message.replyTo!)}>
                    Jump to parent
                  </button>
                )}
                {onOpenThread && (
                  <button
                    className="link"
                    onClick={() => onOpenThread(message.threadId)}
                    disabled={isActiveThread}
                  >
                    {isActiveThread ? 'In thread' : 'Open thread'}
                  </button>
                )}
              </div>
            )}
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
          </div>
        </div>
        {menuPosition && (
          <GroupMessageMenu
            message={message}
            position={menuPosition}
            onClose={() => setMenuPosition(null)}
            onStartThread={onStartThread}
            onOpenThread={onOpenThread}
            onJumpToParent={onJumpToParent}
            onReply={onReply}
            onEdit={onEdit}
            onDelete={onDelete}
            onForward={onForward}
            onReact={onReact}
            isActiveThread={isActiveThread}
            currentNode={currentNode}
          />
        )}
      </>
    );
  });

export default GroupMessage;
