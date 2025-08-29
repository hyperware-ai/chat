import React, { useEffect, useState } from 'react';
import { useChatStore } from '../../store/chat';
import ChatListItem from './ChatListItem';
import ChatSearch from './ChatSearch';
import NewChatButton from './NewChatButton';
import NewChatModal from './NewChatModal';
import './ChatList.css';

const ChatList: React.FC = () => {
  const { chats, loadChats, searchChats } = useChatStore();
  const [searchQuery, setSearchQuery] = useState('');
  const [showNewChat, setShowNewChat] = useState(false);
  const [filteredChats, setFilteredChats] = useState(chats);

  useEffect(() => {
    loadChats();
  }, [loadChats]);

  useEffect(() => {
    if (searchQuery) {
      searchChats(searchQuery).then(setFilteredChats);
    } else {
      setFilteredChats(chats);
    }
  }, [searchQuery, chats, searchChats]);

  return (
    <div className="chat-list-container">
      <div className="chat-list-header">
        <ChatSearch value={searchQuery} onChange={setSearchQuery} />
        <NewChatButton onClick={() => setShowNewChat(true)} />
      </div>
      
      <div className="chat-list">
        {filteredChats.length > 0 ? (
          filteredChats.map(chat => (
            <ChatListItem key={chat.id} chat={chat} />
          ))
        ) : (
          <div className="empty-state">
            {searchQuery ? 'No chats found' : 'No chats yet. Start a new conversation!'}
          </div>
        )}
      </div>
      
      {showNewChat && (
        <NewChatModal onClose={() => setShowNewChat(false)} />
      )}
    </div>
  );
};

export default ChatList;