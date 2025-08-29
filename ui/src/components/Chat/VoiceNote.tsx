import React, { useState } from 'react';
import './VoiceNote.css';

interface VoiceNoteProps {
  onClose: () => void;
}

const VoiceNote: React.FC<VoiceNoteProps> = ({ onClose }) => {
  const [isRecording, setIsRecording] = useState(false);
  const [recordingTime] = useState(0);

  const startRecording = () => {
    setIsRecording(true);
    // TODO: Implement actual recording
    console.log('Starting voice recording');
  };

  const stopRecording = () => {
    setIsRecording(false);
    // TODO: Implement actual recording stop and send
    console.log('Stopping voice recording');
    onClose();
  };

  return (
    <div className="voice-note-overlay" onClick={onClose}>
      <div className="voice-note-modal" onClick={(e) => e.stopPropagation()}>
        {isRecording ? (
          <>
            <div className="recording-indicator">
              <span className="recording-dot"></span>
              Recording... {recordingTime}s
            </div>
            <button className="stop-button" onClick={stopRecording}>
              Stop & Send
            </button>
          </>
        ) : (
          <>
            <p>Hold to record voice note</p>
            <button className="record-button" onClick={startRecording}>
              🎤 Start Recording
            </button>
          </>
        )}
        <button className="cancel-button" onClick={onClose}>
          Cancel
        </button>
      </div>
    </div>
  );
};

export default VoiceNote;