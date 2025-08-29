// Import and re-export types from generated caller-utils
export type {
  Settings,
  ChatKey,
  UserProfile,
  Chat,
  ChatMessage,
  MessageStatus
} from '../../../target/ui/caller-utils';

// Additional frontend-specific types
export interface WsClientMessage {
  SendMessage?: { chat_id: string; content: string; reply_to?: string };
  Ack?: { message_id: string };
  MarkRead?: { chat_id: string };
  UpdateStatus?: { status: string };
  AuthWithKey?: { chat_key: string };
  BrowserMessage?: { content: string };
  Heartbeat?: null;
}

import type { ChatMessage, Chat, UserProfile } from '../../../target/ui/caller-utils';

export interface WsServerMessage {
  NewMessage?: ChatMessage;
  MessageAck?: { message_id: string };
  StatusUpdate?: { node: string; status: string };
  ChatUpdate?: Chat;
  ProfileUpdate?: { node: string; profile: UserProfile };
  AuthSuccess?: { chat_id: string; history: ChatMessage[] };
  AuthFailed?: { reason: string };
  Heartbeat?: null;
  Error?: { message: string };
}