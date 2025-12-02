import React, { useEffect, useRef, useState } from 'react';
import './GroupMessageInput.css';

interface GroupMessageInputProps {
  onSend: (content: string) => Promise<void>;
  disabled?: boolean;
  disabledReason?: string;
}

const GroupMessageInput: React.FC<GroupMessageInputProps> = ({
  onSend,
  disabled,
  disabledReason,
}) => {
  const [message, setMessage] = useState('');
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const isMobile = /iPhone|iPad|iPod|Android/i.test(navigator.userAgent);

  useEffect(() => {
    if (!disabled) {
      inputRef.current?.focus();
    }
  }, [disabled]);

  const handleSend = async () => {
    const text = message.trim();
    if (!text || disabled) return;
    setMessage('');
    await onSend(text);
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
