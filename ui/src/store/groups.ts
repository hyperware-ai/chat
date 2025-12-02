import { create } from 'zustand';
import { Chat } from '#caller-utils';
import { GroupMessage, NormalizedGroup } from '../types/groups';

type BodyCache = Record<string, string>;

const BODY_CACHE_KEY = 'group-message-bodies';
const BODY_CACHE_LIMIT = 400;

function loadBodyCache(): BodyCache {
  try {
    const raw = localStorage.getItem(BODY_CACHE_KEY);
    return raw ? (JSON.parse(raw) as BodyCache) : {};
  } catch (error) {
    console.warn('[GROUPS] Failed to load body cache', error);
    return {};
  }
}

function persistBodyCache(bodies: BodyCache): BodyCache {
  // Keep only the most recent entries to avoid unbounded growth.
  const entries = Object.entries(bodies);
  const trimmed =
    entries.length > BODY_CACHE_LIMIT
      ? Object.fromEntries(entries.slice(entries.length - BODY_CACHE_LIMIT))
      : bodies;
  try {
    localStorage.setItem(BODY_CACHE_KEY, JSON.stringify(trimmed));
  } catch (error) {
    console.warn('[GROUPS] Failed to persist body cache', error);
  }
  return trimmed;
}

function toMap<T>(
  input:
    | [string, T][]
    | Record<string, T>
    | Map<string, T>
    | null
    | undefined,
): Map<string, T> {
  if (!input) return new Map<string, T>();
  if (input instanceof Map) return new Map(input);
  if (Array.isArray(input)) return new Map<string, T>(input);
  return new Map<string, T>(Object.entries(input));
}

function ensureMetadata(
  groupId: string,
  metadata?: Chat.GroupMetadata | null,
): Chat.GroupMetadata {
  const now = Math.floor(Date.now() / 1000);
  return (
    metadata ?? {
      name: 'New Group',
      description: null,
      avatar: null,
      creator_id: '',
      created_at: now,
      updated_at: now,
      visibility: Chat.GroupVisibility.Private,
      default_role_id: `${groupId}:member`,
      root_thread_id: `${groupId}:thread:root`,
    }
  );
}

function describeAttachments(
  attachments: Chat.AttachmentDescriptor[] | undefined,
): string | null {
  if (!attachments || attachments.length === 0) return null;
  const [first] = attachments;
  if (first.filename) return first.filename;
  if (first.mime_type) return first.mime_type;
  return 'Attachment';
}

function normalizeGroup(
  groupId: string,
  group: Chat.Group,
  bodyCache: BodyCache,
): NormalizedGroup {
  const metadata = ensureMetadata(groupId, group.metadata);
  const roles = toMap<Chat.Role>(group.roles as any);
  const members = toMap<Chat.GroupMember>(group.members as any);
  const threads = toMap<Chat.Thread>(group.threads as any);
  const messagesRaw = toMap<Chat.MessageMeta>(group.messages as any);
  const proposalsRaw = toMap<Chat.MembershipProposal>(
    group.membership_proposals as any,
  );

  const messages: GroupMessage[] = Array.from(messagesRaw.values()).map(
    (meta) => {
      const metaBody = (meta as any).body as string | undefined;
      const content =
        metaBody?.trim?.() ||
        bodyCache[meta.message_id] ||
        describeAttachments(meta.attachments) ||
        'Message payload unavailable';

      return {
        id: meta.message_id,
        threadId: meta.thread_id,
        sender: meta.sender,
        timestamp: meta.timestamp,
        type: meta.message_type,
        replyTo: meta.reply_to ?? null,
        attachments: meta.attachments ?? [],
        content,
        status: 'delivered',
      };
    },
  );

  messages.sort((a, b) => a.timestamp - b.timestamp);

  const rootThreadId =
    metadata.root_thread_id ||
    (threads.size > 0 ? Array.from(threads.keys())[0] : null);
  const proposals = Array.from(proposalsRaw.values());

  return {
    id: groupId,
    metadata,
    roles,
    members,
    threads,
    messages,
    rootThreadId,
    proposals,
  };
}

interface GroupStore {
  groups: Chat.GroupSummary[];
  activeGroupId: string | null;
  activeGroup: NormalizedGroup | null;
  activeThreadId: string | null;
  subscriberEvents: Chat.SubscriberDeliveryEvent[];
  replication: Record<string, Chat.GroupReplicationState>;
  replicationMetrics: Chat.ReplicationMetrics | null;
  isLoading: boolean;
  isSyncing: boolean;
  error: string | null;
  messageBodies: BodyCache;
  loadGroups: () => Promise<void>;
  openGroup: (groupId: string) => Promise<void>;
  refreshActiveGroup: () => Promise<void>;
  createGroup: (input: {
    name: string;
    description?: string;
    visibility?: Chat.GroupVisibility;
    rootThreadTitle?: string | null;
  }) => Promise<string | null>;
  setActiveThread: (threadId: string) => void;
  clearActiveGroup: () => void;
  createThread: (title: string | null) => Promise<string | null>;
  sendMessage: (content: string) => Promise<void>;
  fetchSubscriberEvents: (clear?: boolean) => Promise<void>;
  fetchReplicationState: (groupId?: string | null) => Promise<void>;
  inviteMember: (candidate: string, roleId: string) => Promise<Chat.MembershipDecision | null>;
  approveProposal: (proposalId: string) => Promise<Chat.MembershipDecision | null>;
}

export const useGroupStore = create<GroupStore>((set, get) => ({
  groups: [],
  activeGroupId: null,
  activeGroup: null,
  activeThreadId: null,
  subscriberEvents: [],
  replication: {},
  replicationMetrics: null,
  isLoading: false,
  isSyncing: false,
  error: null,
  messageBodies: loadBodyCache(),

  loadGroups: async () => {
    try {
      set({ isLoading: true });
      const res = await Chat.list_groups();
      const sorted = [...res.groups].sort((a, b) => {
        const aTs = a.metadata?.updated_at ?? 0;
        const bTs = b.metadata?.updated_at ?? 0;
        return bTs - aTs;
      });
      set({ groups: sorted, error: null });
    } catch (error) {
      console.error('[GROUPS] Failed to load groups', error);
      set({ error: 'Failed to load groups' });
    } finally {
      set({ isLoading: false });
    }
  },

  openGroup: async (groupId: string) => {
    try {
      set({ isLoading: true, activeGroupId: groupId });
      const res = await Chat.get_group({ group_id: groupId });
      if (!res.group) {
        set({ error: 'Group not found', activeGroup: null, activeThreadId: null });
        return;
      }

      const normalized = normalizeGroup(groupId, res.group, get().messageBodies);
      const activeThreadId =
        normalized.rootThreadId ||
        normalized.messages[normalized.messages.length - 1]?.threadId ||
        null;

      set({
        activeGroup: normalized,
        activeThreadId,
        error: null,
      });
      // Refresh delivery/replication state alongside opening the group.
      get().fetchReplicationState(groupId);
      get().fetchSubscriberEvents();
    } catch (error) {
      console.error('[GROUPS] Failed to open group', error);
      set({ error: 'Failed to load group', activeGroup: null, activeThreadId: null });
    } finally {
      set({ isLoading: false });
    }
  },

  refreshActiveGroup: async () => {
    const groupId = get().activeGroupId;
    if (!groupId) return;
    try {
      set({ isSyncing: true });
      const res = await Chat.get_group({ group_id: groupId });
      if (!res.group) return;

      const normalized = normalizeGroup(groupId, res.group, get().messageBodies);
      set((state) => ({
        activeGroup: normalized,
        activeThreadId:
          state.activeThreadId && normalized.threads.has(state.activeThreadId)
            ? state.activeThreadId
            : normalized.rootThreadId,
      }));
      get().fetchReplicationState(groupId);
    } catch (error) {
      console.error('[GROUPS] Failed to refresh group', error);
      set({ error: 'Failed to refresh group' });
    } finally {
      set({ isSyncing: false });
    }
  },

  createGroup: async (input) => {
    try {
      set({ isLoading: true });
      const payload: Chat.CreateGroupReq = {
        group_id: null,
        name: input.name,
        description: input.description ?? null,
        avatar: null,
        visibility: input.visibility ?? Chat.GroupVisibility.Private,
        default_role_label: null,
        membership_rules: [],
        root_thread_title: input.rootThreadTitle ?? null,
      };

      const res = await Chat.create_group(payload);
      await get().loadGroups();
      await get().openGroup(res.group_id);
      return res.group_id;
    } catch (error) {
      console.error('[GROUPS] Failed to create group', error);
      set({ error: 'Failed to create group' });
      return null;
    } finally {
      set({ isLoading: false });
    }
  },

  setActiveThread: (threadId: string) => set({ activeThreadId: threadId }),

  clearActiveGroup: () =>
    set({
      activeGroup: null,
      activeGroupId: null,
      activeThreadId: null,
      isSyncing: false,
    }),

  createThread: async (title) => {
    const groupId = get().activeGroupId;
    const parentThreadId = get().activeGroup?.rootThreadId ?? null;
    if (!groupId) return null;
    try {
      const res = await Chat.create_group_thread({
        group_id: groupId,
        parent_thread_id: parentThreadId,
        title: title || null,
      });
      await get().refreshActiveGroup();
      set({ activeThreadId: res.thread_id });
      return res.thread_id;
    } catch (error) {
      console.error('[GROUPS] Failed to create thread', error);
      set({ error: 'Unable to create thread' });
      return null;
    }
  },

  sendMessage: async (content: string) => {
    const groupId = get().activeGroupId;
    const threadId = get().activeThreadId;
    const sender = (window as any).our?.node || 'me';
    if (!groupId || !threadId) return;

    const timestamp = Math.floor(Date.now() / 1000);
    const tempId = `temp-${timestamp}-${Math.random().toString(16).slice(2)}`;
    const optimistic: GroupMessage = {
      id: tempId,
      threadId,
      sender,
      timestamp,
      type: Chat.MessageType.Text,
      replyTo: null,
      attachments: [],
      content,
      status: 'sending' as const,
      isLocal: true,
    };

    set((state) => {
      if (!state.activeGroup) return state;
      const updatedBodies = persistBodyCache({
        ...state.messageBodies,
        [tempId]: content,
      });
      return {
        activeGroup: {
          ...state.activeGroup,
          messages: [...state.activeGroup.messages, optimistic],
        },
        messageBodies: updatedBodies,
        error: null,
      };
    });

    try {
      const res = await Chat.send_group_message({
        group_id: groupId,
        thread_id: threadId,
        content,
        message_type: Chat.MessageType.Text,
        reply_to: null,
        attachments: [],
      });
      const realId = res.message.message_id;
      set((state) => {
        if (!state.activeGroup) return state;
        const updatedBodies = persistBodyCache({
          ...state.messageBodies,
          [realId]: content,
        });
        const messages = state.activeGroup.messages.map((msg) =>
          msg.id === tempId
            ? { ...msg, id: realId, status: 'sent' as const }
            : msg,
        );
        return {
          activeGroup: { ...state.activeGroup, messages },
          messageBodies: updatedBodies,
        };
      });
      // Pull a fresh copy so we pick up any server-side mutations.
      get().refreshActiveGroup();
    } catch (error) {
      console.error('[GROUPS] Failed to send message', error);
      set((state) => {
        if (!state.activeGroup) return state;
        return {
          activeGroup: {
            ...state.activeGroup,
            messages: state.activeGroup.messages.map((msg) =>
              msg.id === tempId ? { ...msg, status: 'failed' as const } : msg,
            ),
          },
          error: 'Failed to send message',
        };
      });
    }
  },

  fetchSubscriberEvents: async (clear = false) => {
    try {
      const res = await Chat.admin_subscriber_events({
        clear,
        take: 50,
      });
      set({ subscriberEvents: res.events });
    } catch (error) {
      console.error('[GROUPS] Failed to fetch subscriber events', error);
    }
  },

  fetchReplicationState: async (groupId: string | null = null) => {
    try {
      const res = await Chat.admin_replication_state({ group_id: groupId });
      const replication = { ...get().replication };
      res.groups.forEach((g) => {
        replication[g.group_id] = g;
      });
      set({
        replication,
        replicationMetrics: res.metrics,
      });
    } catch (error) {
      console.error('[GROUPS] Failed to fetch replication state', error);
    }
  },

  inviteMember: async (candidate, roleId) => {
    const groupId = get().activeGroupId;
    if (!groupId) return null;
    try {
      const res = await Chat.invite_group_member({
        group_id: groupId,
        candidate,
        role_id: roleId,
      });
      await get().refreshActiveGroup();
      return res.decision;
    } catch (error) {
      console.error('[GROUPS] Failed to invite member', error);
      set({ error: 'Failed to invite member' });
      return null;
    }
  },

  approveProposal: async (proposalId) => {
    const groupId = get().activeGroupId;
    if (!groupId) return null;
    try {
      const res = await Chat.approve_group_membership({
        group_id: groupId,
        proposal_id: proposalId,
      });
      await get().refreshActiveGroup();
      return res.decision;
    } catch (error) {
      console.error('[GROUPS] Failed to approve membership', error);
      set({ error: 'Failed to approve membership' });
      return null;
    }
  },
}));
