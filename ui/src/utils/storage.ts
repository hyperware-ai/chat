import { Chat, ChatMessage } from '../types/chat';

interface StoredChatState {
  chats: Chat[];
  lastSyncTimestamp: number;
  messageHashes: Record<string, string>; // chatId -> hash of message IDs
}

class BrowserStorage {
  private readonly STORAGE_KEY = 'chat_state';
  private readonly STORAGE_VERSION = '1.0';

  // Store chat state in localStorage
  saveChatState(chats: Chat[]): void {
    try {
      const messageHashes: Record<string, string> = {};
      
      // Create hashes for each chat's messages for quick comparison
      chats.forEach(chat => {
        const messageIds = chat.messages.map(m => m.id).join(',');
        messageHashes[chat.id] = this.hashString(messageIds);
      });

      const state: StoredChatState = {
        chats: chats.map(chat => ({
          ...chat,
          // Store only essential message data to save space
          messages: chat.messages.slice(-50) // Keep last 50 messages per chat
        })),
        lastSyncTimestamp: Date.now(),
        messageHashes
      };

      const serialized = JSON.stringify({
        version: this.STORAGE_VERSION,
        state
      });

      // Use compression if available (for larger datasets)
      localStorage.setItem(this.STORAGE_KEY, serialized);
    } catch (error) {
      console.error('Failed to save chat state:', error);
      // Clear storage if quota exceeded
      if (error instanceof DOMException && error.name === 'QuotaExceededError') {
        this.clearStorage();
      }
    }
  }

  // Load chat state from localStorage
  loadChatState(): StoredChatState | null {
    try {
      const serialized = localStorage.getItem(this.STORAGE_KEY);
      if (!serialized) return null;

      const parsed = JSON.parse(serialized);
      
      // Check version compatibility
      if (parsed.version !== this.STORAGE_VERSION) {
        this.clearStorage();
        return null;
      }

      return parsed.state;
    } catch (error) {
      console.error('Failed to load chat state:', error);
      this.clearStorage();
      return null;
    }
  }

  // Generate diff between stored and server state
  generateDiff(storedChats: Chat[], serverChats: Chat[]): ChatStateDiff {
    const diff: ChatStateDiff = {
      newChats: [],
      updatedChats: [],
      deletedChatIds: [],
      newMessages: {},
      updatedMessages: {},
      deletedMessageIds: {}
    };

    const storedChatsMap = new Map(storedChats.map(c => [c.id, c]));
    const serverChatsMap = new Map(serverChats.map(c => [c.id, c]));

    // Find new and updated chats
    serverChats.forEach(serverChat => {
      const storedChat = storedChatsMap.get(serverChat.id);
      
      if (!storedChat) {
        diff.newChats.push(serverChat);
      } else {
        // Check if chat metadata changed
        if (this.hasChatMetadataChanged(storedChat, serverChat)) {
          diff.updatedChats.push(serverChat);
        }

        // Find message differences
        const messageDiff = this.getMessageDiff(storedChat.messages, serverChat.messages);
        if (messageDiff.new.length > 0) {
          diff.newMessages[serverChat.id] = messageDiff.new;
        }
        if (messageDiff.updated.length > 0) {
          diff.updatedMessages[serverChat.id] = messageDiff.updated;
        }
        if (messageDiff.deleted.length > 0) {
          diff.deletedMessageIds[serverChat.id] = messageDiff.deleted;
        }
      }
    });

    // Find deleted chats
    storedChats.forEach(storedChat => {
      if (!serverChatsMap.has(storedChat.id)) {
        diff.deletedChatIds.push(storedChat.id);
      }
    });

    return diff;
  }

  // Apply diff to stored state
  applyDiff(storedChats: Chat[], diff: ChatStateDiff): Chat[] {
    const chatsMap = new Map(storedChats.map(c => [c.id, c]));

    // Remove deleted chats
    diff.deletedChatIds.forEach(id => chatsMap.delete(id));

    // Add new chats
    diff.newChats.forEach(chat => chatsMap.set(chat.id, chat));

    // Update existing chats
    diff.updatedChats.forEach(updatedChat => {
      const existingChat = chatsMap.get(updatedChat.id);
      if (existingChat) {
        chatsMap.set(updatedChat.id, {
          ...existingChat,
          ...updatedChat,
          messages: existingChat.messages // Keep messages, they're handled separately
        });
      }
    });

    // Apply message changes
    Object.entries(diff.newMessages).forEach(([chatId, messages]) => {
      const chat = chatsMap.get(chatId);
      if (chat) {
        chat.messages = [...chat.messages, ...messages].sort((a, b) => a.timestamp - b.timestamp);
      }
    });

    Object.entries(diff.updatedMessages).forEach(([chatId, messages]) => {
      const chat = chatsMap.get(chatId);
      if (chat) {
        const messageMap = new Map(chat.messages.map(m => [m.id, m]));
        messages.forEach(msg => messageMap.set(msg.id, msg));
        chat.messages = Array.from(messageMap.values()).sort((a, b) => a.timestamp - b.timestamp);
      }
    });

    Object.entries(diff.deletedMessageIds).forEach(([chatId, messageIds]) => {
      const chat = chatsMap.get(chatId);
      if (chat) {
        const idsToDelete = new Set(messageIds);
        chat.messages = chat.messages.filter(m => !idsToDelete.has(m.id));
      }
    });

    return Array.from(chatsMap.values());
  }

  // Get only changes since last sync
  getChangesSinceLastSync(timestamp: number): SyncRequest {
    const stored = this.loadChatState();
    if (!stored) {
      return { fullSync: true };
    }

    return {
      fullSync: false,
      lastSyncTimestamp: stored.lastSyncTimestamp,
      messageHashes: stored.messageHashes
    };
  }

  private hasChatMetadataChanged(stored: Chat, server: Chat): boolean {
    return stored.counterparty !== server.counterparty ||
           stored.is_blocked !== server.is_blocked ||
           stored.notify !== server.notify ||
           stored.unread_count !== server.unread_count;
  }

  private getMessageDiff(storedMessages: ChatMessage[], serverMessages: ChatMessage[]) {
    const storedMap = new Map(storedMessages.map(m => [m.id, m]));
    const serverMap = new Map(serverMessages.map(m => [m.id, m]));

    const newMessages: ChatMessage[] = [];
    const updatedMessages: ChatMessage[] = [];
    const deletedIds: string[] = [];

    // Find new and updated messages
    serverMessages.forEach(serverMsg => {
      const storedMsg = storedMap.get(serverMsg.id);
      if (!storedMsg) {
        newMessages.push(serverMsg);
      } else if (this.hasMessageChanged(storedMsg, serverMsg)) {
        updatedMessages.push(serverMsg);
      }
    });

    // Find deleted messages
    storedMessages.forEach(storedMsg => {
      if (!serverMap.has(storedMsg.id)) {
        deletedIds.push(storedMsg.id);
      }
    });

    return {
      new: newMessages,
      updated: updatedMessages,
      deleted: deletedIds
    };
  }

  private hasMessageChanged(stored: ChatMessage, server: ChatMessage): boolean {
    return stored.content !== server.content ||
           stored.status !== server.status ||
           JSON.stringify(stored.reactions) !== JSON.stringify(server.reactions);
  }

  private hashString(str: string): string {
    let hash = 0;
    for (let i = 0; i < str.length; i++) {
      const char = str.charCodeAt(i);
      hash = ((hash << 5) - hash) + char;
      hash = hash & hash; // Convert to 32-bit integer
    }
    return hash.toString(36);
  }

  clearStorage(): void {
    localStorage.removeItem(this.STORAGE_KEY);
  }
}

interface ChatStateDiff {
  newChats: Chat[];
  updatedChats: Chat[];
  deletedChatIds: string[];
  newMessages: Record<string, ChatMessage[]>;
  updatedMessages: Record<string, ChatMessage[]>;
  deletedMessageIds: Record<string, string[]>;
}

interface SyncRequest {
  fullSync: boolean;
  lastSyncTimestamp?: number;
  messageHashes?: Record<string, string>;
}

export const browserStorage = new BrowserStorage();
export type { StoredChatState, ChatStateDiff, SyncRequest };