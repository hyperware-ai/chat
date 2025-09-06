import React, { useState } from 'react';
import './FileUpload.css';
import { useChatStore } from '../../store/chat';
import { upload_file } from '../../../../target/ui/caller-utils';

interface FileUploadProps {
  onClose: () => void;
}

const FileUpload: React.FC<FileUploadProps> = ({ onClose }) => {
  const { activeChat, loadChats, settings } = useChatStore();
  const [isUploading, setIsUploading] = useState(false);
  const [uploadProgress, setUploadProgress] = useState<{ [filename: string]: number }>({});
  
  const handleFileSelect = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const files = e.target.files;
    if (files && files.length > 0 && activeChat) {
      const maxSizeBytes = (settings.max_file_size_mb || 10) * 1024 * 1024;
      setIsUploading(true);
      
      for (const file of Array.from(files)) {
        try {
          // Check file size
          if (file.size > maxSizeBytes) {
            alert(`File "${file.name}" exceeds the ${settings.max_file_size_mb || 10}MB size limit`);
            continue;
          }
          
          // Set initial progress
          setUploadProgress(prev => ({ ...prev, [file.name]: 0 }));
          
          // Read file as base64
          const reader = new FileReader();
          
          reader.onprogress = (event) => {
            if (event.lengthComputable) {
              const percentComplete = (event.loaded / event.total) * 100;
              setUploadProgress(prev => ({ ...prev, [file.name]: percentComplete * 0.5 })); // 50% for reading
            }
          };
          
          reader.onload = async (event) => {
            if (event.target?.result) {
              const base64 = (event.target.result as string).split(',')[1]; // Remove data URL prefix
              
              // Update progress to show uploading
              setUploadProgress(prev => ({ ...prev, [file.name]: 50 }));
              
              // Upload file
              await upload_file({
                chat_id: activeChat.id,
                filename: file.name,
                mime_type: file.type || 'application/octet-stream',
                data: base64,
                reply_to: null
              });
              
              // Mark as complete
              setUploadProgress(prev => ({ ...prev, [file.name]: 100 }));
              
              await loadChats(); // Refresh to show new message
              
              // Remove from progress after a delay
              setTimeout(() => {
                setUploadProgress(prev => {
                  const newProgress = { ...prev };
                  delete newProgress[file.name];
                  return newProgress;
                });
              }, 1000);
            }
          };
          
          reader.readAsDataURL(file);
        } catch (error) {
          console.error('Error uploading file:', error);
          // Remove failed upload from progress
          setUploadProgress(prev => {
            const newProgress = { ...prev };
            delete newProgress[file.name];
            return newProgress;
          });
        }
      }
      
      // Close dialog after all uploads complete
      setTimeout(() => {
        setIsUploading(false);
        if (Object.keys(uploadProgress).length === 0) {
          onClose();
        }
      }, 1500);
    }
  };

  return (
    <div className="file-upload-overlay" onClick={onClose}>
      <div className="file-upload-menu" onClick={(e) => e.stopPropagation()}>
        {/* Show upload progress if uploading */}
        {Object.keys(uploadProgress).length > 0 && (
          <div className="upload-progress-container">
            {Object.entries(uploadProgress).map(([filename, progress]) => (
              <div key={filename} className="upload-progress-item">
                <div className="upload-filename">{filename}</div>
                <div className="upload-progress-bar">
                  <div 
                    className="upload-progress-fill" 
                    style={{ width: `${progress}%` }}
                  />
                </div>
                <div className="upload-progress-text">{Math.round(progress)}%</div>
              </div>
            ))}
          </div>
        )}
        
        {/* Show upload options when not uploading */}
        {!isUploading && (
          <>
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
          </>
        )}
      </div>
    </div>
  );
};

export default FileUpload;