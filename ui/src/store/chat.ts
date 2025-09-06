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
import { idbStorage } from '../utils/indexeddb';

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
  loadChatsFromServer: () => Promise<void>;
  syncWithServer: () => Promise<void>;
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

// Track if already initialized to prevent double initialization
let isInitialized = false;

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
    max_file_size_mb: 10,
  },
  chatKeys: [],
  wsConnection: null,
  connectionStatus: 'disconnected',
  error: null,
  isLoading: false,
  replyingTo: null,

  // Initialize the app
  initialize: async () => {
    console.log('[INIT] Starting initialization...');
    
    // Prevent double initialization
    if (isInitialized) {
      console.log('[INIT] Already initialized, skipping...');
      return;
    }
    isInitialized = true;
    
    try {
      // Check if we're connected to Hyperware
      const our = (window as any).our;
      console.log('[INIT] our.node:', our?.node);
      
      if (our?.node) {
        set({ nodeId: our.node, isConnected: true });
        
        // Initialize IndexedDB
        console.log('[INIT] Initializing IndexedDB...');
        await idbStorage.init();
        
        // Load cached chats from IndexedDB
        console.log('[INIT] Loading chats from IndexedDB...');
        const cachedChats = await idbStorage.loadChats();
        const activeChatId = await idbStorage.loadMetadata('activeChatId');
        const lastSync = await idbStorage.loadMetadata('lastSyncTimestamp');
        
        if (cachedChats.length > 0) {
          console.log('[INIT] Loaded', cachedChats.length, 'chats from IndexedDB');
          
          // Restore active chat
          let activeChat = null;
          if (activeChatId) {
            activeChat = cachedChats.find(c => c.id === activeChatId) || null;
          }
          
          set({ 
            chats: cachedChats,
            activeChat,
            isLoading: false // Don't show loading since we have cached data
          });
          
          // Check if we need to sync with server
          const ageMs = lastSync ? Date.now() - lastSync : Infinity;
          const ageMinutes = ageMs / (1000 * 60);
          console.log('[INIT] Cache age:', ageMinutes.toFixed(1), 'minutes');
          
          // Sync in background if data is older than 1 minute
          if (ageMinutes > 1) {
            setTimeout(() => get().syncWithServer(), 1000);
          }
        } else {
          console.log('[INIT] No cached data, loading from server...');
          set({ isLoading: true });
          await get().loadChatsFromServer();
        }
        
        // Connect WebSocket for real-time updates
        console.log('[INIT] Connecting WebSocket...');
        get().connectWebSocket();
        
        // Load profile and settings in background
        Promise.all([
          get().loadProfile(),
          get().loadSettings(),
        ]).catch(error => {
          console.error('[INIT] Failed to load profile/settings:', error);
        });
        
      } else {
        set({ isConnected: false, error: 'Not connected to Hyperware' });
      }
    } catch (error) {
      console.error('[INIT] Initialization error:', error);
      set({ error: error instanceof Error ? error.message : 'Failed to initialize' });
    } finally {
      set({ isLoading: false });
    }
  },

  // Load chats from server and save to IndexedDB
  loadChatsFromServer: async () => {
    try {
      console.log('[SYNC] Loading chats from server...');
      const chats = await api.get_chats();
      console.log('[SYNC] Loaded', chats.length, 'chats from server');
      
      set({ chats });
      
      // Save complete data to IndexedDB
      await idbStorage.saveChats(chats);
      
      // Save active chat ID if we have one
      const state = get();
      if (state.activeChat) {
        await idbStorage.saveMetadata('activeChatId', state.activeChat.id);
      }
      
      console.log('[SYNC] Saved chats to IndexedDB');
    } catch (error) {
      console.error('[SYNC] Failed to load chats:', error);
      set({ error: 'Failed to load chats' });
    }
  },

  // Sync with server - much simpler now with IndexedDB
  syncWithServer: async () => {
    try {
      console.log('[SYNC] Syncing with server...');
      
      // Fetch all chats from server (complete history)
      const serverChats = await api.get_chats();
      console.log('[SYNC] Got', serverChats.length, 'chats from server');
      
      // Update state with server data
      set({ chats: serverChats });
      
      // Save everything to IndexedDB
      await idbStorage.saveChats(serverChats);
      
      // Update metadata
      const state = get();
      if (state.activeChat) {
        // Update activeChat with fresh data
        const updatedActiveChat = serverChats.find(c => c.id === state.activeChat?.id);
        if (updatedActiveChat) {
          set({ activeChat: updatedActiveChat });
        }
        await idbStorage.saveMetadata('activeChatId', state.activeChat.id);
      }
      
      console.log('[SYNC] Sync complete, saved to IndexedDB');
    } catch (error) {
      console.error('[SYNC] Failed to sync with server:', error);
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
      const chat = await api.create_chat({ counterparty });
      
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
      const message = await api.send_message({ 
        chat_id: chatId, 
        content, 
        reply_to: replyTo || null,
        file_info: null
      });
      
      // Replace optimistic message with real message from API
      set(state => {
        const updatedChats = state.chats.map(chat => 
          chat.id === chatId 
            ? { 
                ...chat, 
                messages: chat.messages.map(m => 
                  m.id === tempId ? message : m
                ), 
                last_activity: message.timestamp 
              }
            : chat
        );
        
        // Save the updated chat to IndexedDB
        const updatedChat = updatedChats.find(c => c.id === chatId);
        if (updatedChat) {
          idbStorage.saveChat(updatedChat);
        }
        
        return {
          chats: updatedChats,
          activeChat: state.activeChat?.id === chatId 
            ? { 
                ...state.activeChat, 
                messages: state.activeChat.messages.map(m => 
                  m.id === tempId ? message : m
                ) 
              }
            : state.activeChat,
        };
      });
      
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
      const chatId = get().activeChat?.id;
      if (!chatId) throw new Error('No active chat');
      
      await api.edit_message({ 
        chat_id: chatId,
        message_id: messageId, 
        new_content: newContent 
      });
      
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
      const chatId = get().activeChat?.id;
      if (!chatId) throw new Error('No active chat');
      
      await api.delete_message({ 
        chat_id: chatId,
        message_id: messageId 
      });
      
      // Update local state
      set(state => {
        const updatedChats = state.chats.map(chat => ({
          ...chat,
          messages: chat.messages.filter(msg => msg.id !== messageId)
        }));
        
        // Save the updated chat to IndexedDB
        const updatedChat = updatedChats.find(c => c.id === chatId);
        if (updatedChat) {
          idbStorage.saveChat(updatedChat);
        }
        
        return {
          chats: updatedChats,
          // Also update activeChat if it's the same chat
          activeChat: state.activeChat?.id === chatId 
            ? { 
                ...state.activeChat, 
                messages: state.activeChat.messages.filter(msg => msg.id !== messageId) 
              }
            : state.activeChat
        };
      });
    } catch (error) {
      set({ error: 'Failed to delete message' });
    }
  },

  // Delete a chat
  deleteChat: async (chatId: string) => {
    try {
      await api.delete_chat({ chat_id: chatId });
      
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
      await api.update_settings(settings);
      set({ settings });
    } catch (error) {
      set({ error: 'Failed to update settings' });
    }
  },

  // Update profile
  updateProfile: async (profile: UserProfile) => {
    try {
      await api.update_profile(profile);
      set({ profile });
    } catch (error) {
      set({ error: 'Failed to update profile' });
    }
  },

  // Search chats
  searchChats: async (query: string) => {
    try {
      return await api.search_chats({ query });
    } catch (error) {
      set({ error: 'Failed to search chats' });
      return [];
    }
  },

  // Set active chat
  setActiveChat: (chat: Chat | null) => {
    set({ activeChat: chat });
    // Save active chat ID to IndexedDB
    if (chat) {
      idbStorage.saveMetadata('activeChatId', chat.id);
    } else {
      idbStorage.saveMetadata('activeChatId', null);
    }
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
          const existingChat = state.chats[existingChatIndex];
          
          // Merge messages intelligently to preserve optimistic messages
          const mergedMessages = [...updatedChat.messages];
          
          // Check for any optimistic messages (temp IDs) that aren't in the update
          existingChat.messages.forEach(msg => {
            if (msg.id.startsWith('temp-')) {
              // Keep optimistic messages that haven't been replaced yet
              const hasRealVersion = updatedChat.messages.some(m => 
                m.content === msg.content && 
                m.sender === msg.sender &&
                Math.abs(m.timestamp - msg.timestamp) < 5 // Within 5 seconds
              );
              if (!hasRealVersion) {
                mergedMessages.push(msg);
              }
            }
          });
          
          // Sort messages by timestamp
          mergedMessages.sort((a, b) => a.timestamp - b.timestamp);
          
          newChats = [...state.chats];
          newChats[existingChatIndex] = {
            ...updatedChat,
            messages: mergedMessages
          };
        } else {
          // Add new chat
          console.log('[WS] Adding new chat');
          newChats = [...state.chats, updatedChat];
        }
        
        // Update activeChat if it's the same chat being updated
        let updatedActiveChat = state.activeChat;
        if (state.activeChat && state.activeChat.id === updatedChat.id) {
          console.log('[WS] Updating activeChat with new data');
          const existingChat = state.chats[existingChatIndex];
          
          // Same merging logic for activeChat
          const mergedMessages = [...updatedChat.messages];
          if (existingChat) {
            existingChat.messages.forEach(msg => {
              if (msg.id.startsWith('temp-')) {
                const hasRealVersion = updatedChat.messages.some(m => 
                  m.content === msg.content && 
                  m.sender === msg.sender &&
                  Math.abs(m.timestamp - msg.timestamp) < 5
                );
                if (!hasRealVersion) {
                  mergedMessages.push(msg);
                }
              }
            });
          }
          mergedMessages.sort((a, b) => a.timestamp - b.timestamp);
          
          updatedActiveChat = {
            ...updatedChat,
            messages: mergedMessages
          };
        }
        
        // Save updated chat to IndexedDB
        const changedChat = newChats.find(c => c.id === updatedChat.id);
        if (changedChat) {
          idbStorage.saveChat(changedChat);
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
          
          // Save updated chat to IndexedDB
          if (foundChat) {
            const chatToSave = updatedChats.find(c => 
              c.counterparty === newMsg.sender || c.id.includes(newMsg.sender)
            );
            if (chatToSave) {
              idbStorage.saveChat(chatToSave);
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
      const chatId = get().activeChat?.id;
      if (!chatId) throw new Error('No active chat');
      
      return await api.create_chat_link({ 
        chat_id: chatId,
        single_use: singleUse 
      });
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
      await api.revoke_chat_key({ key });
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