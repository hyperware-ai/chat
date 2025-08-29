import React from 'react';
import './FileUpload.css';

interface FileUploadProps {
  onClose: () => void;
}

const FileUpload: React.FC<FileUploadProps> = ({ onClose }) => {
  const handleFileSelect = (e: React.ChangeEvent<HTMLInputElement>) => {
    const files = e.target.files;
    if (files && files.length > 0) {
      // TODO: Implement file upload
      console.log('Files selected:', files);
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