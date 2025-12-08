import React, { useMemo, useRef, useEffect } from 'react';
import { Chat } from '#caller-utils';
import GroupMessage from './GroupMessage';
import GroupMessageInput from './GroupMessageInput';
import { GroupMessage as GroupMessageType } from '../../types/groups';
import './ThreadView.css';

type ThreadWithId = Chat.Thread & { id: string };

interface ReplyingToMessage {
  id: string;
  sender: string;
  content: string;
}

interface DraftThread {
  parentThreadId: string;
  rootMessageId: string | null;
  title: string | null;
}

interface EditingMessage {
  id: string;
  content: string;
}

interface ThreadViewProps {
  threadId: string | null;
  threads: ThreadWithId[];
  messages: GroupMessageType[];
  currentNode: string | null | undefined;
  canSend: boolean;
  disabledReason?: string;
  canStartThread: boolean;
  replyingTo?: ReplyingToMessage | null;
  draftThread?: DraftThread | null;
  editingMessage?: EditingMessage | null;
  onSend: (content: string, replyTo?: string | null) => Promise<void>;
  onStartThread?: (parentThreadId: string, rootMessageId?: string) => void;
  onOpenThread: (threadId: string) => void;
  onReply?: (messageId: string) => void;
  onCancelReply?: () => void;
  onSetEditingMessage?: (message: EditingMessage) => void;
  onCancelEdit?: () => void;
  onEdit?: (messageId: string, newContent: string) => void;
  onDelete?: (messageId: string) => void;
  onReact?: (messageId: string, emoji: string) => void;
}

const ThreadView: React.FC<ThreadViewProps> = ({
  threadId,
  threads,
  messages,
  currentNode,
  canSend,
  disabledReason,
  canStartThread,
  replyingTo,
  draftThread,
  editingMessage,
  onSend,
  onStartThread,
  onOpenThread,
  onReply,
  onCancelReply,
  onSetEditingMessage,
  onCancelEdit,
  onEdit,
  onDelete,
  onReact,
}) => {
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const messageRefs = useRef(new Map<string, HTMLDivElement>());

  const threadMeta = useMemo(() => threads.find((t) => t.id === threadId), [threads, threadId]);
  const threadMap = useMemo(() => {
    const map = new Map<string, ThreadWithId>();
    threads.forEach((t) => map.set(t.id, t));
    return map;
  }, [threads]);
  const rootThreadId = useMemo(
    () => threads.find((t) => t.depth === 0)?.id ?? null,
    [threads],
  );
  const threadPath = useMemo(() => {
    if (!threadId) return [];
    const path: ThreadWithId[] = [];
    let current: ThreadWithId | undefined | null = threadMap.get(threadId);
    while (current) {
      path.unshift(current);
      const parentRef = current.parent as any;
      if (parentRef && 'Thread' in parentRef) {
        const parentId = parentRef.Thread as string;
        current = threadMap.get(parentId);
      } else {
        current = null;
      }
    }
    return path;
  }, [threadId, threadMap]);

  const filtered = useMemo(() => {
    if (!threadId) return [];
    // Only include messages that belong to this thread
    // Don't include the root message here since it's shown separately
    return messages.filter((m) => m.threadId === threadId);
  }, [messages, threadId]);

  const rootMessage = useMemo(() => {
    if (!threadMeta?.root_message_id) return null;
    return messages.find((m) => m.id === threadMeta.root_message_id) || null;
  }, [messages, threadMeta?.root_message_id]);

  // Map message IDs to threads that have them as root (for showing thread indicators)
  // Exclude the current thread to avoid showing "has thread" for messages in their own thread
  const messageToChildThread = useMemo(() => {
    const map = new Map<string, { id: string; title: string | null }>();
    threads.forEach((t) => {
      if (t.root_message_id && t.id !== threadId) {
        map.set(t.root_message_id, { id: t.id, title: t.title || null });
      }
    });
    return map;
  }, [threads, threadId]);

  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [filtered.length, threadId]);

  const registerMessageRef = (id: string, el: HTMLDivElement | null) => {
    if (!el) {
      messageRefs.current.delete(id);
    } else {
      messageRefs.current.set(id, el);
    }
  };

  // Draft thread mode - show empty thread view ready for first message
  if (draftThread && !threadId) {
    const parentThread = threads.find((t) => t.id === draftThread.parentThreadId);
    const draftRootMessage = draftThread.rootMessageId
      ? messages.find((m) => m.id === draftThread.rootMessageId)
      : null;

    return (
      <div className="thread-view">
        <div className="thread-header">
          <div className="thread-crumbs">
            {parentThread && (
              <>
                <button className="crumb-link" onClick={() => onOpenThread(parentThread.id)}>
                  {parentThread.depth === 0 ? 'Main thread' : parentThread.title || `Thread`}
                </button>
                <span className="crumb-sep">/</span>
              </>
            )}
            <span className="crumb-current crumb-draft">New Thread</span>
          </div>
          <div className="thread-sub">Draft • Send a message to create</div>
        </div>

        <div className="thread-messages">
          {draftRootMessage && (
            <div className="thread-root-message">
              <GroupMessage
                key={`root-${draftRootMessage.id}`}
                message={draftRootMessage}
                currentNode={currentNode}
                onOpenThread={undefined}
                onStartThread={undefined}
                onReact={onReact}
                isActiveThread={false}
                allMessages={messages}
              />
            </div>
          )}
          <div className="thread-empty">Start this thread by sending a message below.</div>
        </div>

        <GroupMessageInput
          onSend={onSend}
          onEdit={onEdit}
          disabled={!canSend}
          disabledReason={disabledReason}
          replyingTo={replyingTo}
          onCancelReply={onCancelReply}
          editingMessage={editingMessage}
          onCancelEdit={onCancelEdit}
        />
      </div>
    );
  }

  if (!threadId) {
    return <div className="thread-view thread-empty">Select a thread to view its messages.</div>;
  }

  return (
    <div className="thread-view">
      <div className="thread-header">
        <div className="thread-crumbs">
          {threadPath.length === 0 ? (
            <button
              className="crumb-link"
              onClick={() => rootThreadId && onOpenThread(rootThreadId)}
              disabled={!rootThreadId || threadMeta?.depth === 0}
            >
              Main thread
            </button>
          ) : (
            threadPath.map((crumb, idx) => {
              const isLast = idx === threadPath.length - 1;
              const label = crumb.depth === 0 ? 'Main thread' : crumb.title || `Thread ${crumb.id}`;
              return (
                <React.Fragment key={crumb.id}>
                  {idx > 0 && <span className="crumb-sep">/</span>}
                  {isLast ? (
                    <span className="crumb-current">{label}</span>
                  ) : (
                    <button className="crumb-link" onClick={() => onOpenThread(crumb.id)}>
                      {label}
                    </button>
                  )}
                </React.Fragment>
              );
            })
          )}
        </div>
        <div className="thread-sub">
          Depth {threadMeta?.depth ?? 0} •{' '}
          {threadMeta?.summary?.message_count ?? filtered.length} msgs
        </div>
      </div>

      <div className="thread-messages">
        {/* Show root message at the top if it exists and is from a different thread */}
        {rootMessage && rootMessage.threadId !== threadId && (
          <div className="thread-root-message">
            <GroupMessage
              key={`root-${rootMessage.id}`}
              message={rootMessage}
              currentNode={currentNode}
              onOpenThread={undefined}
              onStartThread={undefined}
              onReact={onReact}
              isActiveThread={false}
              allMessages={messages}
            />
          </div>
        )}

        {filtered.length === 0 ? (
          <div className="thread-empty">No messages yet. Start the conversation!</div>
        ) : (
          filtered.map((msg) => (
            <GroupMessage
              key={msg.id}
              ref={(el) => registerMessageRef(msg.id, el)}
              message={msg}
              currentNode={currentNode}
              onOpenThread={onOpenThread}
              onStartThread={
                canStartThread && onStartThread
                  ? (parentId, rootMsgId) => onStartThread(parentId, rootMsgId)
                  : undefined
              }
              onReply={onReply}
              onSetEditingMessage={onSetEditingMessage}
              onDelete={onDelete}
              onReact={onReact}
              isActiveThread={false}
              childThread={messageToChildThread.get(msg.id) || null}
              allMessages={messages}
            />
          ))
        )}
        <div ref={messagesEndRef} />
      </div>

      <GroupMessageInput
        onSend={onSend}
        onEdit={onEdit}
        disabled={!canSend}
        disabledReason={disabledReason}
        replyingTo={replyingTo}
        onCancelReply={onCancelReply}
        editingMessage={editingMessage}
        onCancelEdit={onCancelEdit}
      />
    </div>
  );
};

export default ThreadView;
