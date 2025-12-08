import React, { useMemo, useState } from 'react';
import { GroupMessage } from '../../types/groups';
import '../Chat/MessageMenu.css';

interface GroupMessageMenuProps {
  message: GroupMessage;
  position: { x: number; y: number };
  onClose: () => void;
  onStartThread?: (parentThreadId: string, rootMessageId?: string) => void | Promise<void>;
  onOpenThread?: (threadId: string) => void;
  onJumpToParent?: (parentId: string) => void;
  onReply?: (messageId: string) => void;
  onEdit?: (messageId: string, content: string) => void;
  onDelete?: (messageId: string) => void;
  onForward?: (messageId: string) => void;
  onReact?: (messageId: string, emoji: string) => void;
  isActiveThread?: boolean;
  currentNode?: string | null;
}

const GroupMessageMenu: React.FC<GroupMessageMenuProps> = ({
  message,
  position,
  onClose,
  onStartThread,
  onOpenThread,
  onJumpToParent,
  onReply,
  onEdit,
  onDelete,
  onForward,
  onReact,
  isActiveThread,
  currentNode,
}) => {
  const [showEmojiPicker, setShowEmojiPicker] = useState(false);
  const menuStyle = useMemo(() => {
    // Rough menu sizing based on how many options we render.
    const optionCount = [
      Boolean(onJumpToParent && message.replyTo),
      true, // Copy always present
    ].filter(Boolean).length + 5; // Reply, Forward, React, Edit, Delete

    const menuHeight = Math.max(140, optionCount * 44);
    const menuWidth = 170;
    const padding = 10;

    let top = position.y;
    let left = position.x;

    if (top + menuHeight > window.innerHeight - padding) {
      top = Math.max(padding, position.y - menuHeight);
    }
    if (left + menuWidth > window.innerWidth - padding) {
      left = window.innerWidth - menuWidth - padding;
    }

    top = Math.max(padding, top);
    left = Math.max(padding, left);

    return { top, left };
  }, [position, onJumpToParent, message.replyTo]);

  const handleCopy = () => {
    if (navigator.clipboard) {
      navigator.clipboard.writeText(message.content).catch(() => {});
    }
    onClose();
  };

  const handleReply = () => {
    if (onReply) {
      onReply(message.id);
      onClose();
    }
  };

  const handleEdit = () => {
    if (onEdit) {
      const newContent = window.prompt('Edit message:', message.content);
      if (newContent && newContent !== message.content) {
        onEdit(message.id, newContent);
      }
      onClose();
    }
  };

  const handleDelete = () => {
    if (onDelete) {
      if (window.confirm('Are you sure you want to delete this message?')) {
        onDelete(message.id);
      }
      onClose();
    }
  };

  const handleForward = () => {
    if (onForward) {
      onForward(message.id);
      onClose();
    }
  };

  const handleReact = () => {
    if (!onReact) return;
    setShowEmojiPicker(true);
  };

  const isMyMessage = currentNode && message.sender === currentNode;
  const commonEmojis = ['👍', '❤️', '😂', '😮', '😢', '😡', '👎', '⚡', '🔥', '💯'];

  return (
    <>
      <div className="menu-overlay" onClick={onClose} />
      {showEmojiPicker ? (
        <div
          className="emoji-tray"
          style={{
            position: 'fixed',
            top: Math.min(position.y, window.innerHeight - 100),
            left: Math.min(position.x, window.innerWidth - 400),
            transform: 'translateY(-50%)',
          }}
        >
          {commonEmojis.map((emoji) => (
            <button
              key={emoji}
              className="emoji-tray-option"
              onClick={() => {
                if (onReact) {
                  onReact(message.id, emoji);
                }
                onClose();
              }}
            >
              {emoji}
            </button>
          ))}
          <button
            className="emoji-tray-more"
            onClick={() => {
              setShowEmojiPicker(false);
            }}
          >
            Cancel
          </button>
        </div>
      ) : (
        <div className="message-menu" style={menuStyle}>
          <button onClick={handleReply} disabled={!onReply}>
            Reply
          </button>
          <button onClick={handleForward} disabled={!onForward}>
            Forward
          </button>
          <button onClick={handleCopy}>Copy</button>
          <button onClick={handleReact} disabled={!onReact}>
            React
          </button>
          <button onClick={handleEdit} disabled={!onEdit || !isMyMessage}>
            Edit
          </button>
          <button onClick={handleDelete} disabled={!onDelete || !isMyMessage}>
            Delete
          </button>
          {onJumpToParent && message.replyTo && (
            <button
              onClick={() => {
                onJumpToParent(message.replyTo!);
                onClose();
              }}
            >
              Jump to parent
            </button>
          )}
        </div>
      )}
    </>
  );
};

export default GroupMessageMenu;
