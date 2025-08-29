import { create } from 'zustand';
import { 
  Chat, 
  UserProfile, 
  Settings, 
  ChatKey 
} from '../types/chat';
import type { WsServerMessage } from '../types/chat';
import * as api from '../utils/chatApi';
import { ChatWebSocket } from '../utils/websocket';

interface ChatStore {
  // State
  nodeId: string | null;
  isConnected: boolean;
  profile: UserProfile | null;
  chats: Chat[];
  activeChat: Chat | null;
  settings: Settings;
  chatKeys: ChatKey[];
  wsConnection: ChatWebSocket | null;
  connectionStatus: 'connected' | 'disconnected' | 'connecting';
  error: string | null;
  isLoading: boolean;
  replyingTo: any | null; // Message being replied to
  
  // Actions
  initialize: () => Promise<void>;
  loadChats: () => Promise<void>;
  loadProfile: () => Promise<void>;
  loadSettings: () => Promise<void>;
  createChat: (counterparty: string) => Promise<void>;
  sendMessage: (chatId: string, content: string, replyTo?: string) => Promise<void>;
  editMessage: (messageId: string, newContent: string) => Promise<void>;
  deleteMessage: (messageId: string) => Promise<void>;
  deleteChat: (chatId: string) => Promise<void>;
  updateSettings: (settings: Settings) => Promise<void>;
  updateProfile: (profile: UserProfile) => Promise<void>;
  searchChats: (query: string) => Promise<Chat[]>;
  setActiveChat: (chat: Chat | null) => void;
  markChatAsRead: (chatId: string) => Promise<void>;
  connectWebSocket: () => void;
  disconnectWebSocket: () => void;
  handleWebSocketMessage: (message: WsServerMessage) => void;
  createChatLink: (singleUse: boolean) => Promise<string>;
  loadChatKeys: () => Promise<void>;
  revokeChatKey: (key: string) => Promise<void>;
  setError: (error: string | null) => void;
  clearError: () => void;
  setReplyingTo: (message: any | null) => void;
}

export const useChatStore = create<ChatStore>((set, get) => ({
  // Initial state
  nodeId: null,
  isConnected: false,
  profile: null,
  chats: [],
  activeChat: null,
  settings: {
    showImages: true,
    showProfilePics: true,
    combineChatsGroups: false,
    notifyChats: true,
    notifyGroups: true,
    notifyCalls: true,
    allowBrowserChats: true,
    sttEnabled: false,
    sttApiKey: null,
  },
  chatKeys: [],
  wsConnection: null,
  connectionStatus: 'disconnected',
  error: null,
  isLoading: false,
  replyingTo: null,

  // Initialize the app
  initialize: async () => {
    try {
      set({ isLoading: true });
      
      // Check if we're connected to Hyperware
      const our = (window as any).our;
      if (our?.node) {
        set({ nodeId: our.node, isConnected: true });
        
        // Load initial data
        await Promise.all([
          get().loadProfile(),
          get().loadSettings(),
          get().loadChats(),
        ]);
        
        // Connect WebSocket
        get().connectWebSocket();
      } else {
        set({ isConnected: false, error: 'Not connected to Hyperware' });
      }
    } catch (error) {
      set({ error: error instanceof Error ? error.message : 'Failed to initialize' });
    } finally {
      set({ isLoading: false });
    }
  },

  // Load all chats
  loadChats: async () => {
    try {
      const chats = await api.getChats();
      set({ chats });
    } catch (error) {
      set({ error: 'Failed to load chats' });
    }
  },

  // Load user profile
  loadProfile: async () => {
    try {
      const profile = await api.getProfile();
      set({ profile });
    } catch (error) {
      set({ error: 'Failed to load profile' });
    }
  },

  // Load settings
  loadSettings: async () => {
    try {
      const settings = await api.getSettings();
      set({ settings });
    } catch (error) {
      set({ error: 'Failed to load settings' });
    }
  },

  // Create a new chat
  createChat: async (counterparty: string) => {
    try {
      set({ isLoading: true });
      const requestBody = JSON.stringify({ counterparty });
      const chat = await api.createChat(requestBody);
      
      set(state => ({
        chats: [chat, ...state.chats],
        activeChat: chat,
      }));
    } catch (error) {
      set({ error: 'Failed to create chat' });
    } finally {
      set({ isLoading: false });
    }
  },

  // Send a message
  sendMessage: async (chatId: string, content: string, replyTo?: string) => {
    try {
      const requestBody = JSON.stringify({ 
        chat_id: chatId, 
        content, 
        reply_to: replyTo 
      });
      const message = await api.sendMessage(requestBody);
      
      // Update local state with the message from API (which has the correct status)
      set(state => ({
        chats: state.chats.map(chat => 
          chat.id === chatId 
            ? { ...chat, messages: [...chat.messages, message], lastActivity: message.timestamp }
            : chat
        ),
        activeChat: state.activeChat?.id === chatId 
          ? { ...state.activeChat, messages: [...state.activeChat.messages, message] }
          : state.activeChat,
      }));
      
      // Don't send via WebSocket - the HTTP endpoint already handles P2P messaging
    } catch (error) {
      set({ error: 'Failed to send message' });
    }
  },

  // Edit a message
  editMessage: async (messageId: string, newContent: string) => {
    try {
      const requestBody = JSON.stringify({ 
        message_id: messageId, 
        new_content: newContent 
      });
      await api.editMessage(requestBody);
      
      // Update local state
      set(state => ({
        chats: state.chats.map(chat => ({
          ...chat,
          messages: chat.messages.map(msg => 
            msg.id === messageId ? { ...msg, content: newContent } : msg
          )
        }))
      }));
    } catch (error) {
      set({ error: 'Failed to edit message' });
    }
  },

  // Delete a message
  deleteMessage: async (messageId: string) => {
    try {
      const requestBody = JSON.stringify({ message_id: messageId });
      await api.deleteMessage(requestBody);
      
      // Update local state
      set(state => ({
        chats: state.chats.map(chat => ({
          ...chat,
          messages: chat.messages.filter(msg => msg.id !== messageId)
        }))
      }));
    } catch (error) {
      set({ error: 'Failed to delete message' });
    }
  },

  // Delete a chat
  deleteChat: async (chatId: string) => {
    try {
      const requestBody = JSON.stringify({ chat_id: chatId });
      await api.deleteChat(requestBody);
      
      set(state => ({
        chats: state.chats.filter(chat => chat.id !== chatId),
        activeChat: state.activeChat?.id === chatId ? null : state.activeChat,
      }));
    } catch (error) {
      set({ error: 'Failed to delete chat' });
    }
  },

  // Update settings
  updateSettings: async (settings: Settings) => {
    try {
      const requestBody = JSON.stringify(settings);
      await api.updateSettings(requestBody);
      set({ settings });
    } catch (error) {
      set({ error: 'Failed to update settings' });
    }
  },

  // Update profile
  updateProfile: async (profile: UserProfile) => {
    try {
      const requestBody = JSON.stringify(profile);
      await api.updateProfile(requestBody);
      set({ profile });
    } catch (error) {
      set({ error: 'Failed to update profile' });
    }
  },

  // Search chats
  searchChats: async (query: string) => {
    try {
      const requestBody = JSON.stringify({ query });
      return await api.searchChats(requestBody);
    } catch (error) {
      set({ error: 'Failed to search chats' });
      return [];
    }
  },

  // Set active chat
  setActiveChat: (chat: Chat | null) => {
    set({ activeChat: chat });
  },

  // Mark chat as read
  markChatAsRead: async (chatId: string) => {
    set(state => ({
      chats: state.chats.map(chat => 
        chat.id === chatId ? { ...chat, unreadCount: 0 } : chat
      )
    }));
    
    // Send via WebSocket if connected
    const ws = get().wsConnection;
    if (ws) {
      ws.send({ MarkRead: { chat_id: chatId } });
    }
  },

  // WebSocket connection
  connectWebSocket: () => {
    const ws = new ChatWebSocket();
    
    ws.connect((message: WsServerMessage) => {
      get().handleWebSocketMessage(message);
    });
    
    set({ 
      wsConnection: ws, 
      connectionStatus: 'connecting' 
    });
  },

  disconnectWebSocket: () => {
    const ws = get().wsConnection;
    if (ws) {
      ws.disconnect();
      set({ wsConnection: null, connectionStatus: 'disconnected' });
    }
  },

  handleWebSocketMessage: (message: WsServerMessage) => {
    if (message.ChatUpdate) {
      // Handle new chat or chat update
      const updatedChat = message.ChatUpdate;
      set(state => {
        const existingChatIndex = state.chats.findIndex(c => c.id === updatedChat.id);
        if (existingChatIndex >= 0) {
          // Update existing chat
          const newChats = [...state.chats];
          newChats[existingChatIndex] = updatedChat;
          return { chats: newChats };
        } else {
          // Add new chat
          return { chats: [...state.chats, updatedChat] };
        }
      });
    } else if (message.NewMessage) {
      const newMsg = message.NewMessage;
      const our = (window as any).our;
      
      // Only add the message if we didn't send it (prevents duplicates)
      if (newMsg.sender !== our?.node) {
        set(state => {
          const updatedChats = state.chats.map(chat => {
            // Find the chat this message belongs to
            const isRelevantChat = chat.counterparty === newMsg.sender || 
                                  chat.id.includes(newMsg.sender);
            if (isRelevantChat) {
              // Check if message already exists to prevent duplicates
              const messageExists = chat.messages.some(m => m.id === newMsg.id);
              if (!messageExists) {
                return {
                  ...chat,
                  messages: [...chat.messages, newMsg],
                  lastActivity: newMsg.timestamp,
                  unreadCount: chat.id !== state.activeChat?.id ? chat.unreadCount + 1 : chat.unreadCount
                };
              }
            }
            return chat;
          });
          
          // Also update activeChat if it's the same chat
          let updatedActiveChat = state.activeChat;
          if (state.activeChat && (state.activeChat.counterparty === newMsg.sender || 
                                   state.activeChat.id.includes(newMsg.sender))) {
            const messageExists = state.activeChat.messages.some(m => m.id === newMsg.id);
            if (!messageExists) {
              updatedActiveChat = {
                ...state.activeChat,
                messages: [...state.activeChat.messages, newMsg],
                lastActivity: newMsg.timestamp
              };
            }
          }
          
          return {
            chats: updatedChats,
            activeChat: updatedActiveChat
          };
        });
      }
    } else if (message.MessageAck) {
      const { message_id } = message.MessageAck;
      set(state => ({
        chats: state.chats.map(chat => ({
          ...chat,
          messages: chat.messages.map(msg => 
            msg.id === message_id ? { ...msg, status: 'Delivered' as const } : msg
          )
        })),
        activeChat: state.activeChat ? {
          ...state.activeChat,
          messages: state.activeChat.messages.map(msg =>
            msg.id === message_id ? { ...msg, status: 'Delivered' as const } : msg
          )
        } : null
      }));
    } else if (message.StatusUpdate) {
      // Handle status updates
      set({ connectionStatus: 'connected' });
    } else if (message.Heartbeat) {
      // Handle heartbeat
      set({ connectionStatus: 'connected' });
    }
  },

  // Browser chat management
  createChatLink: async (singleUse: boolean) => {
    try {
      const requestBody = JSON.stringify({ single_use: singleUse });
      return await api.createChatLink(requestBody);
    } catch (error) {
      set({ error: 'Failed to create chat link' });
      throw error;
    }
  },

  loadChatKeys: async () => {
    try {
      const chatKeys = await api.getChatKeys();
      set({ chatKeys });
    } catch (error) {
      set({ error: 'Failed to load chat keys' });
    }
  },

  revokeChatKey: async (key: string) => {
    try {
      const requestBody = JSON.stringify({ key });
      await api.revokeChatKey(requestBody);
      await get().loadChatKeys();
    } catch (error) {
      set({ error: 'Failed to revoke chat key' });
    }
  },

  // Error handling
  setError: (error: string | null) => set({ error }),
  clearError: () => set({ error: null }),
  
  // Reply functionality
  setReplyingTo: (message: any | null) => set({ replyingTo: message }),
}));