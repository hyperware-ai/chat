import React, { useEffect, useRef, useState } from 'react';
import GroupFileUpload from './GroupFileUpload';
import './GroupMessageInput.css';

interface ReplyingToMessage {
  id: string;
  sender: string;
  content: string;
}

interface EditingMessage {
  id: string;
  content: string;
}

interface GroupMessageInputProps {
  onSend: (content: string, replyTo?: string | null) => Promise<void>;
  onEdit?: (messageId: string, newContent: string) => void;
  disabled?: boolean;
  disabledReason?: string;
  replyingTo?: ReplyingToMessage | null;
  onCancelReply?: () => void;
  editingMessage?: EditingMessage | null;
  onCancelEdit?: () => void;
}

const GroupMessageInput: React.FC<GroupMessageInputProps> = ({
  onSend,
  onEdit,
  disabled,
  disabledReason,
  replyingTo,
  onCancelReply,
  editingMessage,
  onCancelEdit,
}) => {
  const [message, setMessage] = useState('');
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const isMobile = /iPhone|iPad|iPod|Android/i.test(navigator.userAgent);
  const [showFileUpload, setShowFileUpload] = useState(false);

  useEffect(() => {
    if (!disabled) {
      inputRef.current?.focus();
    }
  }, [disabled]);

  // Focus input when replying
  useEffect(() => {
    if (replyingTo) {
      inputRef.current?.focus();
    }
  }, [replyingTo]);

  // Focus input and populate when editing
  useEffect(() => {
    if (editingMessage) {
      setMessage(editingMessage.content);
      inputRef.current?.focus();
    }
  }, [editingMessage]);

  const handleSend = async () => {
    const text = message.trim();
    if (!text || disabled) return;
    setMessage('');

    if (editingMessage && onEdit) {
      // Handle edit
      const editId = editingMessage.id;
      onCancelEdit?.();
      if (text !== editingMessage.content) {
        onEdit(editId, text);
      }
    } else {
      // Handle send
      const replyToId = replyingTo?.id || null;
      onCancelReply?.();
      await onSend(text, replyToId);
    }
    inputRef.current?.focus();
  };

  const handleCancelEdit = () => {
    onCancelEdit?.();
    setMessage('');
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' && !e.shiftKey && !isMobile) {
      e.preventDefault();
      handleSend();
    }
  };

  return (
    <div className="group-message-input">
      {editingMessage && (
        <div className="group-edit-preview">
          <div className="group-edit-info">
            <span className="group-edit-label">Editing message</span>
            <button
              className="group-cancel-edit"
              onClick={handleCancelEdit}
              aria-label="Cancel edit"
            >
              ✕
            </button>
          </div>
          <div className="group-edit-content">{editingMessage.content}</div>
        </div>
      )}
      {replyingTo && !editingMessage && (
        <div className="group-reply-preview">
          <div className="group-reply-info">
            <span className="group-reply-label">Replying to {replyingTo.sender}</span>
            <button
              className="group-cancel-reply"
              onClick={onCancelReply}
              aria-label="Cancel reply"
            >
              ✕
            </button>
          </div>
          <div className="group-reply-content">{replyingTo.content}</div>
        </div>
      )}
      {disabled && disabledReason && (
        <div className="group-input-warning">{disabledReason}</div>
      )}
      <div className={`group-input-row ${disabled ? 'disabled' : ''} ${editingMessage ? 'editing' : ''}`}>
        {!editingMessage && (
          <div className="group-actions">
            <button
              className="group-action-button"
              type="button"
              onClick={() => setShowFileUpload(true)}
              disabled={disabled}
              aria-label="Attach file"
            >
              📎
            </button>
          </div>
        )}
        <textarea
          ref={inputRef}
          placeholder={editingMessage ? 'Edit your message…' : disabled ? 'Sending disabled' : 'Type a message…'}
          value={message}
          onChange={(e) => setMessage(e.target.value)}
          onKeyDown={handleKeyDown}
          rows={1}
          disabled={disabled}
        />
        <button
          className={`group-send ${editingMessage ? 'edit-mode' : ''}`}
          onClick={handleSend}
          disabled={disabled || message.trim().length === 0}
          aria-label={editingMessage ? 'Save edit' : 'Send message'}
        >
          {editingMessage ? '✓' : '➤'}
        </button>
      </div>
      {showFileUpload && <GroupFileUpload onClose={() => setShowFileUpload(false)} />}
    </div>
  );
};

export default GroupMessageInput;
