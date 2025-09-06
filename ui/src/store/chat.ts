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
import { browserStorage } from '../utils/storage';

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
  isLoadingOlderMessages: boolean;
  hasMoreMessages: Record<string, boolean>; // chatId -> boolean
  oldestMessageTimestamp: Record<string, number>; // chatId -> timestamp
  replyingTo: any | null; // Message being replied to
  
  // Actions
  initialize: () => Promise<void>;
  loadChats: () => Promise<void>;
  loadChatsWithDiff: () => Promise<void>;
  syncWithServer: (storedState: any) => Promise<void>;
  loadOlderMessages: (chatId: string) => Promise<void>;
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
    max_file_size_mb: 10,
  },
  chatKeys: [],
  wsConnection: null,
  connectionStatus: 'disconnected',
  error: null,
  isLoading: false,
  isLoadingOlderMessages: false,
  hasMoreMessages: {},
  oldestMessageTimestamp: {},
  replyingTo: null,

  // Initialize the app
  initialize: async () => {
    try {
      set({ isLoading: true });
      
      // Check if we're connected to Hyperware
      const our = (window as any).our;
      if (our?.node) {
        set({ nodeId: our.node, isConnected: true });
        
        // Load initial data - loadChatsWithDiff will handle browser storage
        await Promise.all([
          get().loadProfile(),
          get().loadSettings(),
          get().loadChatsWithDiff(), // This handles both cached state and server sync
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
      // Save to browser storage
      browserStorage.saveChatState(chats);
    } catch (error) {
      set({ error: 'Failed to load chats' });
    }
  },

  // Load chats with diff-based synchronization
  loadChatsWithDiff: async () => {
    try {
      const storedState = browserStorage.loadChatState();
      
      // Check if we have valid stored state
      if (storedState && storedState.chats.length > 0) {
        // Calculate age of stored data
        const ageMs = Date.now() - storedState.lastSyncTimestamp;
        const ageMinutes = ageMs / (1000 * 60);
        
        console.log('[Storage] Using cached state, age:', ageMinutes.toFixed(1), 'minutes');
        
        // Use stored state immediately for instant UI
        set({ 
          chats: storedState.chats,
          hasMoreMessages: {} // Will be updated after server sync
        });
        
        // If data is relatively fresh (< 5 minutes), skip immediate sync
        if (ageMinutes < 5) {
          console.log('[Storage] Cached state is fresh, deferring server sync');
          // Schedule a background sync after a short delay
          setTimeout(async () => {
            await get().syncWithServer(storedState);
          }, 2000);
          return;
        }
        
        // Data is older, sync with server immediately
        await get().syncWithServer(storedState);
      } else {
        // No stored state, fetch from server
        console.log('[Storage] No cached state, fetching from server');
        const serverChats = await api.get_chats();
        
        // Limit initial messages to last 50 per chat for performance
        const limitedChats = serverChats.map(chat => ({
          ...chat,
          messages: chat.messages.slice(-50)
        }));
        
        // Mark chats that might have more messages
        const hasMore: Record<string, boolean> = {};
        serverChats.forEach(chat => {
          hasMore[chat.id] = chat.messages.length > 50;
        });
        
        set({ 
          chats: limitedChats,
          hasMoreMessages: hasMore 
        });
        
        // Save to browser storage
        browserStorage.saveChatState(limitedChats);
      }
    } catch (error) {
      console.error('[Storage] Failed to load chats with diff:', error);
      // Fallback to regular loading
      await get().loadChats();
    }
  },
  
  // Sync stored state with server
  syncWithServer: async (storedState: any) => {
    try {
      console.log('[Storage] Syncing with server...');
      
      // Fetch latest from server
      const serverChats = await api.get_chats();
      
      // Limit messages for performance
      const limitedServerChats = serverChats.map(chat => ({
        ...chat,
        messages: chat.messages.slice(-50)
      }));
      
      // Generate diff between stored and server state
      const diff = browserStorage.generateDiff(storedState.chats, limitedServerChats);
      
      console.log('[Storage] Sync diff stats:', {
        newChats: diff.newChats.length,
        updatedChats: diff.updatedChats.length,
        deletedChats: diff.deletedChatIds.length,
        newMessages: Object.keys(diff.newMessages).length,
        updatedMessages: Object.keys(diff.updatedMessages).length,
      });
      
      // Only update if there are actual changes
      const hasChanges = diff.newChats.length > 0 || 
                        diff.updatedChats.length > 0 || 
                        diff.deletedChatIds.length > 0 ||
                        Object.keys(diff.newMessages).length > 0 ||
                        Object.keys(diff.updatedMessages).length > 0 ||
                        Object.keys(diff.deletedMessageIds).length > 0;
      
      if (hasChanges) {
        // Apply the diff to get the final state
        const mergedChats = browserStorage.applyDiff(storedState.chats, diff);
        
        // Mark chats that might have more messages
        const hasMore: Record<string, boolean> = {};
        serverChats.forEach(chat => {
          hasMore[chat.id] = chat.messages.length > 50;
        });
        
        set({ 
          chats: mergedChats,
          hasMoreMessages: hasMore 
        });
        
        // Save the updated state
        browserStorage.saveChatState(mergedChats);
        console.log('[Storage] Sync complete, state updated');
      } else {
        console.log('[Storage] Sync complete, no changes detected');
        
        // Just update the hasMoreMessages flag
        const hasMore: Record<string, boolean> = {};
        serverChats.forEach(chat => {
          hasMore[chat.id] = chat.messages.length > 50;
        });
        set({ hasMoreMessages: hasMore });
      }
    } catch (error) {
      console.error('[Storage] Failed to sync with server:', error);
    }
  },

  // Load older messages for pagination
  loadOlderMessages: async (chatId: string) => {
    const state = get();
    
    // Don't load if already loading or no more messages
    if (state.isLoadingOlderMessages || state.hasMoreMessages[chatId] === false) {
      return;
    }
    
    try {
      set({ isLoadingOlderMessages: true });
      
      // Get the oldest message timestamp for this chat
      const chat = state.chats.find(c => c.id === chatId);
      if (!chat || chat.messages.length === 0) {
        set({ isLoadingOlderMessages: false });
        return;
      }
      
      const oldestTimestamp = Math.min(...chat.messages.map(m => m.timestamp));
      
      // Load messages before the oldest timestamp using dedicated backend endpoint
      const olderMessages = await api.get_messages({
        chat_id: chatId,
        before_timestamp: oldestTimestamp,
        limit: 50
      });
      
      if (olderMessages.length === 0) {
        // No more messages to load
        set(state => ({
          hasMoreMessages: { ...state.hasMoreMessages, [chatId]: false },
          isLoadingOlderMessages: false
        }));
        return;
      }
      
      // Prepend older messages to the chat
      set(state => {
        const updatedChats = state.chats.map(chat => {
          if (chat.id === chatId) {
            // Merge older messages with existing ones
            const existingIds = new Set(chat.messages.map(m => m.id));
            const newMessages = olderMessages.filter(m => !existingIds.has(m.id));
            
            return {
              ...chat,
              messages: [...newMessages, ...chat.messages]
            };
          }
          return chat;
        });
        
        // Update activeChat if it's the same chat
        let updatedActiveChat = state.activeChat;
        if (state.activeChat?.id === chatId) {
          const existingIds = new Set(state.activeChat.messages.map(m => m.id));
          const newMessages = olderMessages.filter(m => !existingIds.has(m.id));
          
          updatedActiveChat = {
            ...state.activeChat,
            messages: [...newMessages, ...state.activeChat.messages]
          };
        }
        
        // Save to browser storage
        browserStorage.saveChatState(updatedChats);
        
        return {
          chats: updatedChats,
          activeChat: updatedActiveChat,
          isLoadingOlderMessages: false,
          hasMoreMessages: { 
            ...state.hasMoreMessages, 
            [chatId]: olderMessages.length === 50 // If we got full page, there might be more
          },
          oldestMessageTimestamp: {
            ...state.oldestMessageTimestamp,
            [chatId]: Math.min(...olderMessages.map(m => m.timestamp))
          }
        };
      });
    } catch (error) {
      console.error('[Pagination] Failed to load older messages:', error);
      set({ 
        error: 'Failed to load older messages',
        isLoadingOlderMessages: false 
      });
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
        
        // Save to browser storage
        browserStorage.saveChatState(updatedChats);
        
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
        
        // Save to browser storage
        browserStorage.saveChatState(updatedChats);
        
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
        
        // Save updated state to browser storage
        browserStorage.saveChatState(newChats);
        
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
          
          // Save updated state to browser storage
          if (foundChat) {
            browserStorage.saveChatState(updatedChats);
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