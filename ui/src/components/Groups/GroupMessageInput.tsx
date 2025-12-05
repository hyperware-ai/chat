import React, { useEffect, useRef, useState } from 'react';
import './GroupMessageInput.css';

interface ReplyingToMessage {
  id: string;
  sender: string;
  content: string;
}

interface GroupMessageInputProps {
  onSend: (content: string, replyTo?: string | null) => Promise<void>;
  disabled?: boolean;
  disabledReason?: string;
  replyingTo?: ReplyingToMessage | null;
  onCancelReply?: () => void;
}

const GroupMessageInput: React.FC<GroupMessageInputProps> = ({
  onSend,
  disabled,
  disabledReason,
  replyingTo,
  onCancelReply,
}) => {
  const [message, setMessage] = useState('');
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const isMobile = /iPhone|iPad|iPod|Android/i.test(navigator.userAgent);

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

  const handleSend = async () => {
    const text = message.trim();
    if (!text || disabled) return;
    setMessage('');
    const replyToId = replyingTo?.id || null;
    onCancelReply?.(); // Clear reply immediately
    await onSend(text, replyToId);
    inputRef.current?.focus();
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' && !e.shiftKey && !isMobile) {
      e.preventDefault();
      handleSend();
    }
  };

  return (
    <div className="group-message-input">
      {replyingTo && (
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
      <div className={`group-input-row ${disabled ? 'disabled' : ''}`}>
        <textarea
          ref={inputRef}
          placeholder={disabled ? 'Sending disabled' : 'Type a message…'}
          value={message}
          onChange={(e) => setMessage(e.target.value)}
          onKeyDown={handleKeyDown}
          rows={1}
          disabled={disabled}
        />
        <button
          className="group-send"
          onClick={handleSend}
          disabled={disabled || message.trim().length === 0}
          aria-label="Send message"
        >
          ➤
        </button>
      </div>
    </div>
  );
};

export default GroupMessageInput;
