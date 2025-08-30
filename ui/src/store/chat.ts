import { create } from 'zustand';
import { 
  Chat, 
  UserProfile, 
  Settings, 
  ChatKey,
  MessageStatus 
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
    show_images: true,
    show_profile_pics: true,
    combine_chats_groups: false,
    notify_chats: true,
    notify_groups: true,
    notify_calls: true,
    allow_browser_chats: true,
    stt_enabled: false,
    stt_api_key: null,
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
      const chats = await api.get_chats();
      set({ chats });
    } catch (error) {
      set({ error: 'Failed to load chats' });
    }
  },

  // Load user profile
  loadProfile: async () => {
    try {
      const profile = await api.get_profile();
      set({ profile });
    } catch (error) {
      set({ error: 'Failed to load profile' });
    }
  },

  // Load settings
  loadSettings: async () => {
    try {
      const settings = await api.get_settings();
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
      const chat = await api.create_chat(requestBody);
      
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
    // Create optimistic message immediately
    const timestamp = Math.floor(Date.now() / 1000);
    const tempId = `temp-${timestamp}-${Math.random()}`;
    const optimisticMessage = {
      id: tempId,
      sender: (window as any).our?.node || '',
      content,
      timestamp,
      status: 'Sending' as const,
      reply_to: replyTo || null,
      reactions: [],
      message_type: 'Text' as const,
      file_info: null,
    };
    
    // Immediately show the message with "Sending" status
    set(state => ({
      chats: state.chats.map(chat => 
        chat.id === chatId 
          ? { ...chat, messages: [...chat.messages, optimisticMessage], last_activity: timestamp }
          : chat
      ),
      activeChat: state.activeChat?.id === chatId 
        ? { ...state.activeChat, messages: [...state.activeChat.messages, optimisticMessage] }
        : state.activeChat,
    }));
    
    try {
      const requestBody = JSON.stringify({ 
        chat_id: chatId, 
        content, 
        reply_to: replyTo 
      });
      const message = await api.send_message(requestBody);
      
      // Replace optimistic message with real message from API
      set(state => ({
        chats: state.chats.map(chat => 
          chat.id === chatId 
            ? { 
                ...chat, 
                messages: chat.messages.map(m => 
                  m.id === tempId ? message : m
                ), 
                last_activity: message.timestamp 
              }
            : chat
        ),
        activeChat: state.activeChat?.id === chatId 
          ? { 
              ...state.activeChat, 
              messages: state.activeChat.messages.map(m => 
                m.id === tempId ? message : m
              ) 
            }
          : state.activeChat,
      }));
      
    } catch (error) {
      // On error, update the optimistic message to show failed status
      set(state => ({
        chats: state.chats.map(chat => 
          chat.id === chatId 
            ? { 
                ...chat, 
                messages: chat.messages.map(m => 
                  m.id === tempId ? { ...m, status: 'Failed' as const } : m
                )
              }
            : chat
        ),
        activeChat: state.activeChat?.id === chatId 
          ? { 
              ...state.activeChat, 
              messages: state.activeChat.messages.map(m => 
                m.id === tempId ? { ...m, status: 'Failed' as const } : m
              )
            }
          : state.activeChat,
        error: 'Failed to send message'
      }));
    }
  },

  // Edit a message
  editMessage: async (messageId: string, newContent: string) => {
    try {
      const requestBody = JSON.stringify({ 
        message_id: messageId, 
        new_content: newContent 
      });
      await api.edit_message(requestBody);
      
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
      await api.delete_message(requestBody);
      
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
      await api.delete_chat(requestBody);
      
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
      await api.update_settings(requestBody);
      set({ settings });
    } catch (error) {
      set({ error: 'Failed to update settings' });
    }
  },

  // Update profile
  updateProfile: async (profile: UserProfile) => {
    try {
      const requestBody = JSON.stringify(profile);
      await api.update_profile(requestBody);
      set({ profile });
    } catch (error) {
      set({ error: 'Failed to update profile' });
    }
  },

  // Search chats
  searchChats: async (query: string) => {
    try {
      const requestBody = JSON.stringify({ query });
      return await api.search_chats(requestBody);
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
        chat.id === chatId ? { ...chat, unread_count: 0 } : chat
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
    console.log('[WS] Received message:', message);
    
    if (message.ChatUpdate) {
      console.log('[WS] Processing ChatUpdate:', message.ChatUpdate);
      // Handle new chat or chat update
      const updatedChat = message.ChatUpdate;
      console.log('[WS] Chat update received for chat:', updatedChat.id, 'with', updatedChat.messages.length, 'messages');
      
      set(state => {
        const existingChatIndex = state.chats.findIndex(c => c.id === updatedChat.id);
        let newChats;
        
        if (existingChatIndex >= 0) {
          // Update existing chat
          console.log('[WS] Updating existing chat at index:', existingChatIndex);
          newChats = [...state.chats];
          newChats[existingChatIndex] = updatedChat;
        } else {
          // Add new chat
          console.log('[WS] Adding new chat');
          newChats = [...state.chats, updatedChat];
        }
        
        // Update activeChat if it's the same chat being updated
        let updatedActiveChat = state.activeChat;
        if (state.activeChat && state.activeChat.id === updatedChat.id) {
          console.log('[WS] Updating activeChat with new data');
          updatedActiveChat = updatedChat;
        }
        
        return { 
          chats: newChats,
          activeChat: updatedActiveChat
        };
      });
    } else if (message.NewMessage) {
      const newMsg = message.NewMessage;
      console.log('[WS] Processing NewMessage:', newMsg);
      const our = (window as any).our;
      console.log('[WS] Our node:', our?.node, 'Message sender:', newMsg.sender);
      
      // Only add the message if we didn't send it (prevents duplicates)
      if (newMsg.sender !== our?.node) {
        console.log('[WS] Adding message from other node');
        set(state => {
          let foundChat = false;
          const updatedChats = state.chats.map(chat => {
            // Find the chat this message belongs to
            const isRelevantChat = chat.counterparty === newMsg.sender || 
                                  chat.id.includes(newMsg.sender);
            if (isRelevantChat) {
              foundChat = true;
              console.log('[WS] Found chat for message:', chat.id);
              // Check if message already exists to prevent duplicates
              const messageExists = chat.messages.some(m => m.id === newMsg.id);
              if (!messageExists) {
                console.log('[WS] Adding new message to chat');
                return {
                  ...chat,
                  messages: [...chat.messages, newMsg],
                  last_activity: newMsg.timestamp,
                  unread_count: chat.id !== state.activeChat?.id ? chat.unread_count + 1 : chat.unread_count
                };
              } else {
                console.log('[WS] Message already exists, skipping');
              }
            }
            return chat;
          });
          
          if (!foundChat) {
            console.log('[WS] Warning: Could not find chat for message from:', newMsg.sender);
          }
          
          // Also update activeChat if it's the same chat
          let updatedActiveChat = state.activeChat;
          if (state.activeChat && (state.activeChat.counterparty === newMsg.sender || 
                                   state.activeChat.id.includes(newMsg.sender))) {
            const messageExists = state.activeChat.messages.some(m => m.id === newMsg.id);
            if (!messageExists) {
              console.log('[WS] Updating activeChat with new message');
              updatedActiveChat = {
                ...state.activeChat,
                messages: [...state.activeChat.messages, newMsg],
                last_activity: newMsg.timestamp
              };
            }
          }
          
          console.log('[WS] Updated chats after NewMessage. Found:', foundChat);
          console.log('[WS] ActiveChat updated:', updatedActiveChat !== state.activeChat);
          return {
            chats: updatedChats,
            activeChat: updatedActiveChat
          };
        });
      }
    } else if (message.MessageAck) {
      const { message_id } = message.MessageAck;
      console.log('[WS] Processing MessageAck for message:', message_id);
      
      set(state => {
        console.log('[WS] Current chats before ACK update:', state.chats);
        console.log('[WS] Looking for message with ID:', message_id);
        
        const updatedChats = state.chats.map(chat => ({
          ...chat,
          messages: chat.messages.map(msg => {
            if (msg.id === message_id) {
              // MessageAck means BE has received the message, so status should be Sent
              const oldStatus = msg.status;
              const newStatus: MessageStatus = 'Sent';
              console.log(`[WS] Updating message ${message_id} status from ${oldStatus} to ${newStatus}`);
              return { ...msg, status: newStatus };
            }
            return msg;
          })
        }));
        
        // Update activeChat if it contains the message
        let updatedActiveChat = state.activeChat;
        if (state.activeChat) {
          const hasMessage = state.activeChat.messages.some(m => m.id === message_id);
          if (hasMessage) {
            updatedActiveChat = {
              ...state.activeChat,
              messages: state.activeChat.messages.map(msg => {
                if (msg.id === message_id) {
                  const oldStatus = msg.status;
                  const newStatus: MessageStatus = msg.status === 'Sending' ? 'Sent' : 'Delivered';
                  console.log(`[WS] Updating activeChat message ${message_id} status from ${oldStatus} to ${newStatus}`);
                  return { ...msg, status: newStatus };
                }
                return msg;
              })
            };
          }
        }
        
        console.log('[WS] Updated chats after ACK:', updatedChats);
        return {
          chats: updatedChats,
          activeChat: updatedActiveChat
        }
      });
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
      return await api.create_chat_link(requestBody);
    } catch (error) {
      set({ error: 'Failed to create chat link' });
      throw error;
    }
  },

  loadChatKeys: async () => {
    try {
      const chatKeys = await api.get_chat_keys();
      set({ chatKeys });
    } catch (error) {
      set({ error: 'Failed to load chat keys' });
    }
  },

  revokeChatKey: async (key: string) => {
    try {
      const requestBody = JSON.stringify({ key });
      await api.revoke_chat_key(requestBody);
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