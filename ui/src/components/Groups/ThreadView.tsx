import React, { useMemo, useRef, useEffect } from 'react';
import { Chat } from '#caller-utils';
import GroupMessage from './GroupMessage';
import GroupMessageInput from './GroupMessageInput';
import { GroupMessage as GroupMessageType } from '../../types/groups';
import './ThreadView.css';

type ThreadWithId = Chat.Thread & { id: string };

interface ThreadViewProps {
  threadId: string | null;
  threads: ThreadWithId[];
  messages: GroupMessageType[];
  currentNode: string | null | undefined;
  canSend: boolean;
  disabledReason?: string;
  canStartThread: boolean;
  onSend: (content: string) => Promise<void>;
  onStartThread?: (parentThreadId: string) => Promise<void>;
  onOpenThread: (threadId: string) => void;
}

const ThreadView: React.FC<ThreadViewProps> = ({
  threadId,
  threads,
  messages,
  currentNode,
  canSend,
  disabledReason,
  canStartThread,
  onSend,
  onStartThread,
  onOpenThread,
}) => {
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const messageRefs = useRef(new Map<string, HTMLDivElement>());

  const threadMeta = useMemo(() => threads.find((t) => t.id === threadId), [threads, threadId]);
  const rootThreadId = useMemo(
    () => threads.find((t) => t.depth === 0)?.id ?? null,
    [threads],
  );

  const filtered = useMemo(
    () => (threadId ? messages.filter((m) => m.threadId === threadId) : []),
    [messages, threadId],
  );

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

  const handleJumpToParent = (parentId: string) => {
    const target = messageRefs.current.get(parentId);
    if (target) {
      target.scrollIntoView({ behavior: 'smooth', block: 'center' });
    }
  };

  if (!threadId) {
    return <div className="thread-view thread-empty">Select a thread to view its messages.</div>;
  }

  return (
    <div className="thread-view">
      <div className="thread-header">
        <div className="thread-crumbs">
          <button
            className="crumb-link"
            onClick={() => rootThreadId && onOpenThread(rootThreadId)}
            disabled={!rootThreadId || threadMeta?.depth === 0}
          >
            Main thread
          </button>
          {threadMeta?.depth !== undefined && threadMeta.depth > 0 && (
            <>
              <span className="crumb-sep">/</span>
              <span className="crumb-current">
                {threadMeta?.title || `Thread ${threadMeta?.id ?? ''}`}
              </span>
            </>
          )}
        </div>
        <div className="thread-sub">
          Depth {threadMeta?.depth ?? 0} •{' '}
          {threadMeta?.summary?.message_count ?? filtered.length} msgs
        </div>
      </div>

      <div className="thread-messages">
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
                  ? (parentId) => onStartThread(parentId)
                  : undefined
              }
              onJumpToParent={handleJumpToParent}
              isActiveThread={msg.threadId === threadId}
            />
          ))
        )}
        <div ref={messagesEndRef} />
      </div>

      <GroupMessageInput onSend={onSend} disabled={!canSend} disabledReason={disabledReason} />
    </div>
  );
};

export default ThreadView;
