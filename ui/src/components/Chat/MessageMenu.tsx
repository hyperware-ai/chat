import React, { useState } from 'react';
import { ChatMessage } from '../../types/chat';
import { useChatStore } from '../../store/chat';
import { add_reaction, forward_message } from '../../../../target/ui/caller-utils';
import './MessageMenu.css';

interface MessageMenuProps {
  message: ChatMessage;
  isOwn: boolean;
  position: { x: number; y: number };
  onClose: () => void;
}

const MessageMenu: React.FC<MessageMenuProps> = ({ message, isOwn, position, onClose }) => {
  const { deleteMessage, editMessage, loadChats, chats } = useChatStore();
  const [showEmojiPicker, setShowEmojiPicker] = useState(false);
  const [showForwardPicker, setShowForwardPicker] = useState(false);
  
  const commonEmojis = ['👍', '❤️', '😂', '😮', '😢', '😡', '👎', '🎉', '🔥', '💯'];

  const handleReply = () => {
    // Set the message as the one being replied to
    useChatStore.getState().setReplyingTo(message);
    onClose();
  };

  const handleCopy = () => {
    navigator.clipboard.writeText(message.content);
    onClose();
  };

  const handleEdit = () => {
    const newContent = prompt('Edit message:', message.content);
    if (newContent && newContent !== message.content) {
      editMessage(message.id, newContent);
    }
    onClose();
  };

  const handleDelete = () => {
    if (confirm('Delete this message?')) {
      deleteMessage(message.id);
    }
    onClose();
  };
  
  const handleAddReaction = async (emoji: string) => {
    try {
      await add_reaction(JSON.stringify({ 
        message_id: message.id, 
        emoji 
      }));
      await loadChats();
      onClose();
    } catch (err) {
      console.error('Error adding reaction:', err);
    }
  };
  
  const handleForward = async (toChatId: string) => {
    try {
      await forward_message(JSON.stringify({ 
        message_id: message.id, 
        to_chat_id: toChatId 
      }));
      await loadChats();
      onClose();
    } catch (err) {
      console.error('Error forwarding message:', err);
    }
  };

  return (
    <>
      <div className="menu-overlay" onClick={onClose} />
      <div 
        className="message-menu"
        style={{
          top: position.y,
          left: position.x,
        }}
      >
        <button onClick={handleReply}>Reply</button>
        <button onClick={() => setShowForwardPicker(!showForwardPicker)}>Forward</button>
        <button onClick={handleCopy}>Copy</button>
        <button onClick={() => setShowEmojiPicker(!showEmojiPicker)}>React</button>
        {isOwn && <button onClick={handleEdit}>Edit</button>}
        {isOwn && <button onClick={handleDelete}>Delete</button>}
        
        {showEmojiPicker && (
          <div className="emoji-picker">
            {commonEmojis.map(emoji => (
              <button 
                key={emoji} 
                className="emoji-option"
                onClick={() => handleAddReaction(emoji)}
              >
                {emoji}
              </button>
            ))}
          </div>
        )}
        
        {showForwardPicker && (
          <div className="forward-picker">
            <div className="forward-header">Forward to:</div>
            {chats.map(chat => (
              <button 
                key={chat.id}
                className="forward-option"
                onClick={() => handleForward(chat.id)}
              >
                {chat.counterparty}
              </button>
            ))}
          </div>
        )}
      </div>
    </>
  );
};

export default MessageMenu;