import React from 'react';
import { Chat } from '#caller-utils';
import './GroupStatusBar.css';

interface GroupStatusBarProps {
  replication?: Chat.GroupReplicationState;
  events: Chat.SubscriberDeliveryEvent[];
  onRefresh: () => void;
  isSyncing?: boolean;
}

const formatEvent = (event?: Chat.SubscriberDeliveryEvent) => {
  if (!event) return 'No recent delivery events';
  return `${event.kind} • offset ${event.offset} • ${event.age_secs}s ago`;
};

const GroupStatusBar: React.FC<GroupStatusBarProps> = ({
  replication,
  events,
  onRefresh,
  isSyncing,
}) => {
  const lastEvent = events.length > 0 ? events[events.length - 1] : undefined;
  const freshEvent = (lastEvent?.age_secs ?? 999) < 30;

  const state = (() => {
    if (!replication) return { label: 'Waiting for delivery info', tone: 'muted' as const };
    if (replication.pending_bootstrap) return { label: 'Bootstrapping peers…', tone: 'warn' as const };
    if (
      replication.subscriber_lag_secs &&
      replication.subscriber_lag_secs > 45
    ) {
      return {
        label: `Subscriber lane lagging (${replication.subscriber_lag_secs}s)`,
        tone: 'warn' as const,
      };
    }
    return { label: 'Delivery lane healthy', tone: 'ok' as const };
  })();

  return (
    <div className="group-status-bar">
      <div className={`group-status-pill group-status-${state.tone}`}>
        {state.label}
      </div>
      <div className="group-status-details">
        <div className={`group-status-event ${freshEvent ? 'fresh' : ''}`}>
          {formatEvent(lastEvent)}
        </div>
        <button className="group-status-refresh" onClick={onRefresh}>
          {isSyncing ? 'Syncing…' : 'Refresh'}
        </button>
      </div>
    </div>
  );
};

export default GroupStatusBar;
