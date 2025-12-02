import { Chat } from '#caller-utils';

export interface GroupMessage {
  id: string;
  threadId: string;
  sender: string;
  timestamp: number;
  type: Chat.MessageType;
  replyTo: string | null;
  attachments: Chat.AttachmentDescriptor[];
  content: string;
  status?: 'sending' | 'sent' | 'delivered' | 'failed';
  isLocal?: boolean;
}

export interface NormalizedGroup {
  id: string;
  metadata: Chat.GroupMetadata;
  roles: Map<string, Chat.Role>;
  members: Map<string, Chat.GroupMember>;
  threads: Map<string, Chat.Thread>;
  messages: GroupMessage[];
  rootThreadId: string | null;
  proposals: Chat.MembershipProposal[];
}
