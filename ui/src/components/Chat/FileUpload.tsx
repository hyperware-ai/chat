import React from 'react';
import './FileUpload.css';
import { useChatStore } from '../../store/chat';
import { upload_file } from '../../../../target/ui/caller-utils';

interface FileUploadProps {
  onClose: () => void;
}

const FileUpload: React.FC<FileUploadProps> = ({ onClose }) => {
  const { activeChat, loadChats, settings } = useChatStore();
  
  const handleFileSelect = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const files = e.target.files;
    if (files && files.length > 0 && activeChat) {
      const maxSizeBytes = (settings.max_file_size_mb || 10) * 1024 * 1024;
      
      for (const file of Array.from(files)) {
        try {
          // Check file size
          if (file.size > maxSizeBytes) {
            alert(`File "${file.name}" exceeds the ${settings.max_file_size_mb || 10}MB size limit`);
            continue;
          }
          
          // Read file as base64
          const reader = new FileReader();
          reader.onload = async (event) => {
            if (event.target?.result) {
              const base64 = (event.target.result as string).split(',')[1]; // Remove data URL prefix
              
              // Upload file
              const requestBody = JSON.stringify({
                chat_id: activeChat.id,
                filename: file.name,
                mime_type: file.type || 'application/octet-stream',
                data: base64,
                reply_to: null
              });
              
              await upload_file(requestBody);
              await loadChats(); // Refresh to show new message
            }
          };
          reader.readAsDataURL(file);
        } catch (error) {
          console.error('Error uploading file:', error);
        }
      }
      onClose();
    }
  };

  return (
    <div className="file-upload-overlay" onClick={onClose}>
      <div className="file-upload-menu" onClick={(e) => e.stopPropagation()}>
        <button className="upload-option">
          <label htmlFor="file-input">
            📎 Choose File
            <input
              id="file-input"
              type="file"
              onChange={handleFileSelect}
              style={{ display: 'none' }}
              multiple
            />
          </label>
        </button>
        <button className="upload-option">
          <label htmlFor="image-input">
            🖼️ Choose Image
            <input
              id="image-input"
              type="file"
              accept="image/*"
              onChange={handleFileSelect}
              style={{ display: 'none' }}
            />
          </label>
        </button>
        <button className="upload-option" onClick={onClose}>
          Cancel
        </button>
      </div>
    </div>
  );
};

export default FileUpload;