// Re-export all API functions from the generated caller-utils
export {
  create_chat,
  create_chat_link,
  delete_chat,
  delete_message,
  edit_message,
  get_chat,
  get_chat_keys,
  get_chats,
  get_profile,
  get_settings,
  revoke_chat_key,
  search_chats,
  send_message,
  update_profile,
  update_settings,
} from '../../../target/ui/caller-utils';

// Add custom pagination API for loading older messages
export interface GetMessagesReq {
  chat_id: string;
  before_timestamp?: number; // Load messages before this timestamp
  limit?: number; // Number of messages to load (default 50)
}

// For now, we'll use get_chat and filter on the client side
// In a production app, you'd want a dedicated backend endpoint
export async function get_messages_paginated(req: GetMessagesReq) {
  const { get_chat } = await import('../../../target/ui/caller-utils');
  
  // Get the full chat (this is not ideal for large chats)
  const chat = await get_chat({ chat_id: req.chat_id });
  
  // Filter and paginate on client side
  let messages = chat.messages;
  
  if (req.before_timestamp) {
    messages = messages.filter(m => m.timestamp < req.before_timestamp!);
  }
  
  // Sort by timestamp descending and take the limit
  messages.sort((a, b) => b.timestamp - a.timestamp);
  messages = messages.slice(0, req.limit || 50);
  
  // Return in ascending order for display
  return messages.reverse();
}