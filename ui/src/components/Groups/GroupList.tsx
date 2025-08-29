import React from 'react';
import './GroupList.css';

const GroupList: React.FC = () => {
  return (
    <div className="group-list-container">
      <div className="empty-state">
        <span className="empty-icon">👥</span>
        <h3>Groups Coming Soon</h3>
        <p>Group chat functionality will be available in a future update.</p>
      </div>
    </div>
  );
};

export default GroupList;