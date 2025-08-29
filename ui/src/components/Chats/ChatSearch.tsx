import React from 'react';
import './ChatSearch.css';

interface ChatSearchProps {
  value: string;
  onChange: (value: string) => void;
}

const ChatSearch: React.FC<ChatSearchProps> = ({ value, onChange }) => {
  return (
    <div className="chat-search">
      <span className="search-icon">🔍</span>
      <input
        type="text"
        placeholder="Search chats..."
        value={value}
        onChange={(e) => onChange(e.target.value)}
        className="search-input"
      />
    </div>
  );
};

export default ChatSearch;