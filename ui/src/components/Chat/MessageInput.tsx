import React, { useState, useRef, useEffect } from 'react';
import { useChatStore } from '../../store/chat';
import FileUpload from './FileUpload';
import VoiceNote from './VoiceNote';
import './MessageInput.css';

interface MessageInputProps {
  chatId: string;
  onSendMessage?: () => void;
}

const MessageInput: React.FC<MessageInputProps> = ({ chatId, onSendMessage }) => {
  const [message, setMessage] = useState('');
  const [showFileUpload, setShowFileUpload] = useState(false);
  const [showVoiceNote, setShowVoiceNote] = useState(false);
  const { sendMessage, replyingTo, setReplyingTo } = useChatStore();
  const inputRef = useRef<HTMLTextAreaElement>(null);
  
  // Detect if user is on mobile device
  const isMobile = /iPhone|iPad|iPod|Android/i.test(navigator.userAgent) || 
                   ('ontouchstart' in window) || 
                   window.innerWidth <= 768;
  
  // Focus input when replying
  useEffect(() => {
    if (replyingTo) {
      inputRef.current?.focus();
    }
  }, [replyingTo]);

  const handleSubmit = async (e?: React.FormEvent) => {
    e?.preventDefault();
    if (message.trim()) {
      await sendMessage(chatId, message.trim(), replyingTo?.id);
      setMessage('');
      setReplyingTo(null); // Clear reply after sending
      inputRef.current?.focus();
      onSendMessage?.(); // Call callback when message is sent
    }
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    // On desktop: Enter sends, Shift+Enter adds newline
    // On mobile: Enter adds newline, send button must be used
    if (e.key === 'Enter' && !e.shiftKey && !isMobile) {
      e.preventDefault();
      handleSubmit();
    }
  };

  return (
    <div className="message-input-wrapper">
      {replyingTo && (
        <div className="reply-preview">
          <div className="reply-info">
            <span className="reply-label">Replying to {replyingTo.sender}</span>
            <button 
              className="cancel-reply"
              onClick={() => setReplyingTo(null)}
              aria-label="Cancel reply"
            >
              ✕
            </button>
          </div>
          <div className="reply-content">{replyingTo.content}</div>
        </div>
      )}
      
      <div className="message-input-container">
        <button 
          className="attachment-button"
          onClick={() => setShowFileUpload(!showFileUpload)}
          aria-label="Attach file"
        >
          +
        </button>
      
      <textarea
        ref={inputRef}
        className="message-input"
        placeholder={isMobile ? "Type a message... (Enter for newline)" : "Type a message... (Enter to send)"}
        value={message}
        onChange={(e) => setMessage(e.target.value)}
        onKeyDown={handleKeyDown}
        rows={1}
      />
      
      {message.trim() ? (
        <button 
          className="send-button"
          onClick={() => handleSubmit()}
          aria-label="Send message"
        >
          ➤
        </button>
      ) : (
        <button 
          className="voice-button"
          onClick={() => setShowVoiceNote(!showVoiceNote)}
          aria-label="Record voice note"
        >
          🎤
        </button>
      )}
      
        {showFileUpload && <FileUpload onClose={() => setShowFileUpload(false)} />}
        {showVoiceNote && <VoiceNote onClose={() => setShowVoiceNote(false)} />}
      </div>
    </div>
  );
};

export default MessageInput;