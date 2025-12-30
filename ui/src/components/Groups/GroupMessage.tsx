import React, { useMemo, useState, useRef, useEffect } from 'react';
import { GroupMessage as GroupMessageType } from '../../types/groups';
import GroupMessageMenu from './GroupMessageMenu';
import { useChatStore } from '../../store/chat';
import ReactMarkdown from 'react-markdown';
import remarkBreaks from 'remark-breaks';
import remarkHwProtocol from '../../utils/remarkHwProtocol';
import { normalizeMessageContent } from '../../utils/normalizeMessageContent';
import './GroupMessage.css';

interface ChildThreadInfo {
  id: string;
  title: string | null;
}

interface GroupMessageProps {
  message: GroupMessageType;
  currentNode?: string | null;
  onStartThread?: (parentThreadId: string, rootMessageId?: string) => void | Promise<void>;
  onOpenThread?: (threadId: string) => void;
  isActiveThread?: boolean;
  onReply?: (messageId: string) => void;
  onSetEditingMessage?: (message: { id: string; content: string }) => void;
  onDelete?: (messageId: string) => void;
  onReact?: (messageId: string, emoji: string) => void;
  childThread?: ChildThreadInfo | null;
  allMessages?: GroupMessageType[];
}

const formatTime = (timestamp: number) => {
  const date = new Date(timestamp * 1000);
  return date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
};

const GroupMessage = React.forwardRef<HTMLDivElement, GroupMessageProps>(
  ({ message, currentNode, onStartThread, onOpenThread, isActiveThread, onReply, onSetEditingMessage, onDelete, onReact, childThread, allMessages }, ref) => {
    const isMine = currentNode && message.sender === currentNode;
    const statusLabel =
      message.status === 'sending'
        ? 'Sending…'
        : message.status === 'failed'
        ? 'Failed'
        : '';
    const [menuPosition, setMenuPosition] = useState<{ x: number; y: number } | null>(null);
    const longPressTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
    const touchStartRef = useRef<{ x: number; y: number } | null>(null);

    // Clean up timer on unmount
    useEffect(() => {
      return () => {
        if (longPressTimerRef.current) {
          clearTimeout(longPressTimerRef.current);
        }
      };
    }, []);

    const handleContextMenu = (e: React.MouseEvent) => {
      e.preventDefault();
      setMenuPosition({ x: e.clientX, y: e.clientY });
    };

    const groupedReactions = useMemo(() => {
      const grouped: Record<string, string[]> = {};
      message.reactions?.forEach((reaction) => {
        if (!grouped[reaction.emoji]) {
          grouped[reaction.emoji] = [];
        }
        grouped[reaction.emoji].push(reaction.user);
      });
      return grouped;
    }, [message.reactions]);

    const viewerId = currentNode ?? '';
    const { settings } = useChatStore();

    const handleReaction = (emoji: string) => {
      if (onReact) {
        onReact(message.id, emoji);
      }
    };

    const replyToMessage = message.replyTo && allMessages
      ? allMessages.find(m => m.id === message.replyTo)
      : null;

    const handleReplyClick = () => {
      if (message.replyTo) {
        const element = document.getElementById(`group-message-${message.replyTo}`);
        if (element) {
          element.scrollIntoView({ behavior: 'smooth', block: 'center' });
          element.classList.add('highlight');
          setTimeout(() => element.classList.remove('highlight'), 2000);
        }
      }
    };

    // Touch handlers for iOS long press
    const handleTouchStart = (e: React.TouchEvent) => {
      const touch = e.touches[0];
      touchStartRef.current = { x: touch.clientX, y: touch.clientY };

      // Clear any existing timer
      if (longPressTimerRef.current) {
        clearTimeout(longPressTimerRef.current);
      }

      // Start long press timer (500ms)
      longPressTimerRef.current = setTimeout(() => {
        setMenuPosition({ x: touch.clientX, y: touch.clientY });
        // Haptic feedback
        if ('vibrate' in navigator) {
          navigator.vibrate(10);
        }
      }, 500);
    };

    const handleTouchMove = (e: React.TouchEvent) => {
      if (!touchStartRef.current) return;

      const touch = e.touches[0];
      const deltaX = Math.abs(touch.clientX - touchStartRef.current.x);
      const deltaY = Math.abs(touch.clientY - touchStartRef.current.y);

      // Cancel long press if finger moves too much
      if (deltaX > 10 || deltaY > 10) {
        if (longPressTimerRef.current) {
          clearTimeout(longPressTimerRef.current);
          longPressTimerRef.current = null;
        }
      }
    };

    const handleTouchEnd = () => {
      if (longPressTimerRef.current) {
        clearTimeout(longPressTimerRef.current);
        longPressTimerRef.current = null;
      }
      touchStartRef.current = null;
    };

    const normalizedContent = useMemo(
      () => normalizeMessageContent(message.content),
      [message.content],
    );

    const renderMessageContent = useMemo(() => (
      <ReactMarkdown
        remarkPlugins={[remarkBreaks, remarkHwProtocol]}
        urlTransform={(url: string) => {
          if (url.startsWith('hw://')) {
            return url;
          }
          return url;
        }}
        components={{
          a: ({ href, children }) => {
            const imageRegex = /\.(jpg|jpeg|png|gif|webp|svg|bmp)$/i;
            const isHwProtocol = href?.startsWith('hw://');
            const linkColor = isMine ? '#ffffff' : '#4da6ff';

            if (href && imageRegex.test(href) && settings?.show_images) {
              return (
                <div style={{ margin: '8px 0' }}>
                  <a href={href} target="_blank" rel="noopener noreferrer">
                    <img
                      src={href}
                      alt="Image"
                      style={{
                        maxWidth: '100%',
                        maxHeight: '300px',
                        borderRadius: '8px',
                        display: 'block',
                      }}
                      onError={(e) => {
                        const target = e.target as HTMLImageElement;
                        target.style.display = 'none';
                        const link = document.createElement('a');
                        link.href = href;
                        link.target = '_blank';
                        link.rel = 'noopener noreferrer';
                        link.textContent = href;
                        link.style.color = linkColor;
                        link.style.textDecoration = 'underline';
                        target.parentNode?.replaceChild(link, target);
                      }}
                    />
                  </a>
                </div>
              );
            }

            if (isHwProtocol) {
              return (
                <a
                  href={href}
                  style={{
                    color: linkColor,
                    textDecoration: 'underline',
                    cursor: 'pointer',
                  }}
                >
                  {children}
                </a>
              );
            }

            return (
              <a
                href={href}
                target="_blank"
                rel="noopener noreferrer"
                style={{
                  color: linkColor,
                  textDecoration: 'underline',
                }}
              >
                {children}
              </a>
            );
          },
          p: ({ children }) => (
            <p style={{ margin: '4px 0', wordBreak: 'break-word' }}>{children}</p>
          ),
          code: ({ children, ...props }) => {
            const inline = !(
              'className' in props &&
              typeof props.className === 'string' &&
              props.className.includes('language-')
            );
            if (inline) {
              return (
                <code
                  style={{
                    backgroundColor: isMine
                      ? 'rgba(0,0,0,0.2)'
                      : 'rgba(0,0,0,0.1)',
                    padding: '2px 4px',
                    borderRadius: '3px',
                    fontSize: '0.9em',
                  }}
                >
                  {children}
                </code>
              );
            }
            return (
              <pre
                style={{
                  backgroundColor: isMine
                    ? 'rgba(0,0,0,0.2)'
                    : 'rgba(0,0,0,0.1)',
                  padding: '8px',
                  borderRadius: '4px',
                  overflowX: 'auto',
                  fontSize: '0.9em',
                }}
              >
                <code>{children}</code>
              </pre>
            );
          },
          ul: ({ children }) => (
            <ul style={{ margin: '4px 0', paddingLeft: '20px' }}>{children}</ul>
          ),
          ol: ({ children }) => (
            <ol style={{ margin: '4px 0', paddingLeft: '20px' }}>{children}</ol>
          ),
          blockquote: ({ children }) => (
            <blockquote
              style={{
                borderLeft: `3px solid ${
                  isMine ? 'rgba(255,255,255,0.3)' : 'rgba(0,0,0,0.2)'
                }`,
                paddingLeft: '12px',
                margin: '8px 0',
                fontStyle: 'italic',
              }}
            >
              {children}
            </blockquote>
          ),
          h1: ({ children }) => (
            <h1 style={{ fontSize: '1.3em', fontWeight: 'bold', margin: '8px 0 4px 0' }}>
              {children}
            </h1>
          ),
          h2: ({ children }) => (
            <h2 style={{ fontSize: '1.2em', fontWeight: 'bold', margin: '6px 0 4px 0' }}>
              {children}
            </h2>
          ),
          h3: ({ children }) => (
            <h3 style={{ fontSize: '1.1em', fontWeight: 'bold', margin: '4px 0' }}>
              {children}
            </h3>
          ),
          img: ({ src, alt }) => {
            if (!settings?.show_images) return null;
            return (
              <img
                src={src}
                alt={alt}
                style={{
                  maxWidth: '100%',
                  maxHeight: '300px',
                  borderRadius: '8px',
                  display: 'block',
                  margin: '8px 0',
                }}
              />
            );
          },
        }}
      >
        {normalizedContent}
      </ReactMarkdown>
    ), [normalizedContent, settings?.show_images, isMine]);

    return (
      <>
        <div className={`group-message ${isMine ? 'mine' : ''}`} ref={ref} id={`group-message-${message.id}`}>
          {!isMine && <div className="group-message-sender">{message.sender}</div>}
          {replyToMessage && (
            <div className="group-message-reply-to" onClick={handleReplyClick}>
              <div className="group-message-reply-label">↩ Reply to {replyToMessage.sender}</div>
              <div className="group-message-reply-content">{replyToMessage.content}</div>
            </div>
          )}
          <div
            className="group-message-bubble"
            onContextMenu={handleContextMenu}
            onTouchStart={handleTouchStart}
            onTouchMove={handleTouchMove}
            onTouchEnd={handleTouchEnd}
          >
            <div className="group-message-text">{renderMessageContent}</div>
            <div className="group-message-meta">
              <span>{formatTime(message.timestamp)}</span>
              {statusLabel && <span className="group-message-status">{statusLabel}</span>}
            </div>
            {message.reactions && message.reactions.length > 0 && (
              <div className="group-message-reactions">
                {Object.entries(groupedReactions).map(([emoji, users]) => (
                  <button
                    key={emoji}
                    className={`reaction ${
                      users.includes(viewerId) ? 'reacted' : ''
                    }`}
                    onClick={() => handleReaction(emoji)}
                    title={users.join(', ')}
                  >
                    {emoji} {users.length > 1 && users.length}
                  </button>
                ))}
              </div>
            )}
            {childThread && onOpenThread && (
              <button
                type="button"
                className="group-message-thread-indicator has-thread"
                onClick={(e) => {
                  e.preventDefault();
                  e.stopPropagation();
                  onOpenThread(childThread.id);
                }}
              >
                <span className="thread-icon">↳</span>
                <span className="thread-label">Thread</span>
              </button>
            )}
          </div>
        </div>
        {menuPosition && (
          <GroupMessageMenu
            message={message}
            position={menuPosition}
            onClose={() => setMenuPosition(null)}
            onStartThread={onStartThread}
            onOpenThread={onOpenThread}
            onReply={onReply}
            onSetEditingMessage={onSetEditingMessage}
            onDelete={onDelete}
            onReact={onReact}
            currentNode={currentNode}
            childThread={childThread}
          />
        )}
      </>
    );
  });

export default GroupMessage;
