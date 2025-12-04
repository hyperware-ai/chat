import React from 'react';
import { Chat } from '#caller-utils';
import './GroupReplicationPanel.css';

interface GroupReplicationPanelProps {
  replication?: Chat.GroupReplicationState;
  whitelist?: Chat.AdminWhitelistRes | null;
  onRefresh: () => void;
  onFetchWhitelist: () => void;
  isFetchingWhitelist?: boolean;
}

const formatCursor = (cursor: Chat.DeliveryCursor | undefined) => {
  if (!cursor) return '—';
  return `q:${cursor.queue_id} • offset ${cursor.last_offset}`;
};

const valuesOf = <T,>(input: unknown): T[] => {
  if (!input) return [];
  if (Array.isArray(input)) return input as T[];
  if (input instanceof Map) return Array.from(input.values()) as T[];
  if (typeof input === 'object') return Object.values(input as Record<string, T>);
  return [];
};

const entriesOf = <T,>(input: unknown): [string, T][] => {
  if (!input) return [];
  if (Array.isArray(input)) return input as [string, T][];
  if (input instanceof Map) return Array.from(input.entries()) as [string, T][];
  if (typeof input === 'object') return Object.entries(input as Record<string, T>);
  return [];
};

const GroupReplicationPanel: React.FC<GroupReplicationPanelProps> = ({
  replication,
  whitelist,
  onRefresh,
  onFetchWhitelist,
  isFetchingWhitelist,
}) => {
  if (!replication) {
    return (
      <div className="group-replication">
        <div className="group-replication-header">
          <div>
            <h4>Delivery lanes</h4>
            <p className="group-replication-sub">
              Hubs, subscribers, and whitelist entries for this group.
            </p>
          </div>
          <div className="group-replication-actions">
            <button className="secondary" onClick={onRefresh}>
              Refresh lanes
            </button>
            <button
              className="secondary"
              onClick={onFetchWhitelist}
              disabled={isFetchingWhitelist}
            >
              {isFetchingWhitelist ? 'Loading ACL…' : 'Load whitelist'}
            </button>
          </div>
        </div>
        <div className="group-replication-empty">
          Delivery info not loaded yet. Hit refresh to fetch it.
        </div>
      </div>
    );
  }

  const hubs: string[] = valuesOf<string>(replication.hubs);
  const subscribers: string[] = valuesOf<string>(replication.subscribers);
  const hubCount = hubs.length;
  const subscriberCount = subscribers.length;
  const whitelistEntries: Chat.WhitelistEntryDebug[] = valuesOf<Chat.WhitelistEntryDebug>(
    whitelist?.entries,
  );
  const hubCursors: [string, Chat.DeliveryCursor][] = entriesOf<Chat.DeliveryCursor>(
    replication.hub_cursors,
  );
  const subscriberCursors: [string, Chat.DeliveryCursor][] = entriesOf<Chat.DeliveryCursor>(
    replication.subscriber_cursors,
  );

  return (
    <div className="group-replication">
      <div className="group-replication-header">
        <div>
          <h4>Delivery lanes</h4>
          <p className="group-replication-sub">
            Hubs, subscribers, and whitelist entries for this group.
          </p>
        </div>
        <div className="group-replication-actions">
          <button className="secondary" onClick={onRefresh}>
            Refresh lanes
          </button>
          <button
            className="secondary"
            onClick={onFetchWhitelist}
            disabled={isFetchingWhitelist}
          >
            {isFetchingWhitelist ? 'Loading ACL…' : 'Load whitelist'}
          </button>
        </div>
      </div>

      {!replication ? (
        <div className="group-replication-empty">
          Delivery info not loaded yet. Hit refresh to fetch it.
        </div>
      ) : (
        <div className="group-replication-grid">
          <div className="group-replication-card">
            <div className="group-replication-title">Hubs ({hubCount})</div>
            <div className="group-replication-list">
              {hubs.length === 0 ? (
                <div className="group-replication-muted">No hubs yet.</div>
              ) : (
                hubs.map((hub) => (
                  <div key={hub} className="group-replication-row">
                    <span className="mono">{hub}</span>
                  </div>
                ))
              )}
            </div>
            <div className="group-replication-meta">
              Hub lag: {replication.hub_lag_secs ?? '—'}s
            </div>
          </div>

          <div className="group-replication-card">
            <div className="group-replication-title">Subscribers ({subscriberCount})</div>
            <div className="group-replication-list">
              {subscribers.length === 0 ? (
                <div className="group-replication-muted">No subscribers yet.</div>
              ) : (
                subscribers.map((sub) => (
                  <div key={sub} className="group-replication-row">
                    <span className="mono">{sub}</span>
                  </div>
                ))
              )}
            </div>
            <div className="group-replication-meta">
              Subscriber lag: {replication.subscriber_lag_secs ?? '—'}s
            </div>
          </div>

          <div className="group-replication-card">
            <div className="group-replication-title">Cursors</div>
            <div className="group-replication-list">
              {hubCursors.map(([node, cursor]) => (
                <div key={`hub-${node}`} className="group-replication-row">
                  <span className="pill">Hub</span>
                  <span className="mono">{node}</span>
                  <span className="muted">{formatCursor(cursor)}</span>
                </div>
              ))}
              {subscriberCursors.map(([node, cursor]) => (
                <div key={`sub-${node}`} className="group-replication-row">
                  <span className="pill">Sub</span>
                  <span className="mono">{node}</span>
                  <span className="muted">{formatCursor(cursor)}</span>
                </div>
              ))}
              {hubCursors.length === 0 && subscriberCursors.length === 0 && (
                  <div className="group-replication-muted">No cursor data available.</div>
                )}
            </div>
            <div className="group-replication-meta">
              Topics: {replication.routing.hub_topic} / {replication.routing.subscriber_topic}
            </div>
          </div>

          <div className="group-replication-card">
            <div className="group-replication-title">
              Whitelist {whitelist ? `(v${whitelist.version})` : ''}
            </div>
            {whitelist ? (
              <div className="group-replication-list">
                {whitelistEntries.length === 0 ? (
                  <div className="group-replication-muted">No whitelist entries.</div>
                ) : (
                  whitelistEntries.slice(0, 6).map((entry) => (
                    <div key={entry.node} className="group-replication-row column">
                      <div className="mono">{entry.node}</div>
                      <div className="muted">
                        pub:{entry.publish.length} • sub:{entry.subscribe.length}{' '}
                        {entry.expires_at ? `• exp ${entry.expires_at}` : ''}
                      </div>
                    </div>
                  ))
                )}
              </div>
            ) : (
              <div className="group-replication-muted">
                Load whitelist to view ACL entries.
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  );
};

export default GroupReplicationPanel;
