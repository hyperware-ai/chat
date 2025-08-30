# Implementation Plan: Hyperware Chat Application

## Overview

A mobile-first chat application for the Hyperware platform supporting:
1. 1:1 Direct Messages (DMs/Chats)
2. Group chats (Groups) - TODO later
3. 1:1 Voice calls (Calls) - TODO later

The app will use WebSockets for real-time communication, support both node-to-node and node-to-browser chat modes, and implement proper message delivery with acknowledgments and queueing.

## Architecture Overview

### Backend Architecture (Rust/Hyperprocess)

#### Core Components

1. **AppState Structure**
   - User profile (name, profile picture)
   - Chat management (active chats, message history)
   - Connection management (WebSocket connections, online status)
   - Message delivery queue (pending messages for offline nodes)
   - Browser chat sessions (chat keys, browser connections)
   - Settings (global app settings)

2. **WebSocket Architecture**
   - Authenticated WebSocket endpoint at `/ws` for node-to-node communication
   - Unauthenticated WebSocket endpoint at `/public-ws` for browser chats
   - Message types: TextMessage, Ack, Heartbeat, StatusUpdate, ChatKeyAuth
   - Connection tracking via channel_id -> participant mappings

3. **Chat Types**
   - **Node-to-Node**: Full p2p chat with message history on both nodes
   - **Node-to-Browser**: Host-controlled chat with browser clients using chat keys

4. **Message Delivery System**
   - Immediate delivery for online nodes
   - Queue system for offline nodes with periodic retry
   - Message acknowledgment protocol
   - Order preservation for queued messages

### Frontend Architecture (React/TypeScript)

#### UI Structure

1. **Main App (`/ui/src/`)**
   - Splash screen with tabs (Chats, Groups, Calls)
   - Settings management
   - Chat list views
   - Search functionality

2. **Public Chat UI (`/ui/public-chat/`)**
   - Browser-based chat interface
   - Chat key authentication
   - Simplified UI for external users

#### State Management (Zustand)
- Chat list and active chats
- Message history
- Online status tracking
- Settings management
- WebSocket connection state

## Detailed Implementation Steps

### Phase 1: Backend Foundation

#### Step 1.1: Define Core Types and State
```rust
// skeleton-app/src/lib.rs

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ChatMessage {
    pub id: String,
    pub sender: String,
    pub content: String,
    pub timestamp: u64,
    pub status: MessageStatus,
    pub reply_to: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum MessageStatus {
    Sending,
    Sent,
    Delivered,
    Failed,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Chat {
    pub id: String,
    pub counterparty: String,
    pub messages: Vec<ChatMessage>,
    pub last_activity: u64,
    pub unread_count: u32,
    pub is_blocked: bool,
    pub notify: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ChatKey {
    pub key: String,
    pub user_name: String,
    pub created_at: u64,
    pub is_revoked: bool,
    pub chat_id: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct UserProfile {
    pub name: String,
    pub profile_pic: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Settings {
    pub show_images: bool,
    pub show_profile_pics: bool,
    pub combine_chats_groups: bool,
    pub notify_chats: bool,
    pub notify_groups: bool,
    pub notify_calls: bool,
    pub allow_browser_chats: bool,
    pub stt_enabled: bool,
    pub stt_api_key: Option<String>,
}

pub struct AppState {
    pub profile: UserProfile,
    pub chats: HashMap<String, Chat>,
    pub chat_keys: HashMap<String, ChatKey>,
    pub settings: Settings,
    pub delivery_queue: HashMap<String, Vec<ChatMessage>>,
    pub online_nodes: HashSet<String>,
    pub ws_connections: HashMap<u32, String>, // channel_id -> node/browser_id
    pub browser_connections: HashMap<String, u32>, // chat_key -> channel_id
}
```

#### Step 1.2: WebSocket Message Types
```rust
#[derive(Serialize, Deserialize)]
pub enum WsClientMessage {
    // Node-to-node messages
    SendMessage { chat_id: String, content: String, reply_to: Option<String> },
    Ack { message_id: String },
    MarkRead { chat_id: String },
    UpdateStatus { status: String },
    
    // Browser chat messages
    AuthWithKey { chat_key: String },
    BrowserMessage { content: String },
    
    // Common
    Heartbeat,
}

#[derive(Serialize, Deserialize)]
pub enum WsServerMessage {
    // Node-to-node messages
    NewMessage(ChatMessage),
    MessageAck { message_id: String },
    StatusUpdate { node: String, status: String },
    ChatUpdate(Chat),
    
    // Browser chat messages
    AuthSuccess { chat_id: String, history: Vec<ChatMessage> },
    AuthFailed { reason: String },
    
    // Common
    Heartbeat,
    Error { message: String },
}
```

#### Step 1.3: HTTP Endpoints

1. **Chat Management**
   - `#[http] create_chat(counterparty: String) -> Result<Chat>`
   - `#[http] get_chats() -> Vec<Chat>`
   - `#[http] get_chat(chat_id: String) -> Result<Chat>`
   - `#[http] delete_chat(chat_id: String) -> Result<()>`

2. **Message Operations**
   - `#[http] send_message(chat_id: String, content: String, reply_to: Option<String>) -> Result<ChatMessage>`
   - `#[http] edit_message(message_id: String, new_content: String) -> Result<()>`
   - `#[http] delete_message(message_id: String) -> Result<()>`

3. **Browser Chat Management**
   - `#[http] create_chat_link(single_use: bool) -> Result<String>`
   - `#[http] get_chat_keys() -> Vec<ChatKey>`
   - `#[http] revoke_chat_key(key: String) -> Result<()>`

4. **Settings**
   - `#[http] get_settings() -> Settings`
   - `#[http] update_settings(settings: Settings) -> Result<()>`
   - `#[http] update_profile(profile: UserProfile) -> Result<()>`

5. **Search**
   - `#[http] search_chats(query: String) -> Vec<Chat>`

#### Step 1.4: WebSocket Handler Implementation

```rust
#[ws]
async fn handle_ws(&mut self, channel_id: u32, message: String) {
    // Parse message and handle based on type
    // Update connection mappings
    // Broadcast to relevant participants
    // Handle acknowledgments and queuing
}
```

#### Step 1.5: P2P Communication

Implement node-to-node messaging using `send` and `send_rmp`:
- Message delivery with acknowledgments
- Queue management for offline nodes
- Periodic retry mechanism
- Online status tracking

### Phase 2: Frontend Implementation

#### Step 2.1: Main UI Structure

1. **App.tsx** - Main router and layout
   - Tab navigation (Chats, Groups, Calls)
   - Settings modal
   - Profile management

2. **Components Structure**
   ```
   components/
   ├── SplashScreen/
   │   ├── SplashScreen.tsx
   │   ├── TabBar.tsx
   │   └── ProfileButton.tsx
   ├── Chats/
   │   ├── ChatList.tsx
   │   ├── ChatListItem.tsx
   │   ├── ChatSearch.tsx
   │   └── NewChatButton.tsx
   ├── Chat/
   │   ├── ChatView.tsx
   │   ├── MessageList.tsx
   │   ├── Message.tsx
   │   ├── MessageInput.tsx
   │   ├── VoiceNote.tsx
   │   └── FileUpload.tsx
   ├── Settings/
   │   ├── SettingsModal.tsx
   │   ├── ProfileSettings.tsx
   │   ├── ChatSettings.tsx
   │   └── NotificationSettings.tsx
   └── Common/
       ├── Avatar.tsx
       ├── SearchBar.tsx
       └── LoadingSpinner.tsx
   ```

#### Step 2.2: Zustand Store

```typescript
// store/chat.ts
interface ChatStore {
  // State
  profile: UserProfile;
  chats: Chat[];
  activeChat: Chat | null;
  settings: Settings;
  wsConnection: WebSocket | null;
  connectionStatus: 'connected' | 'disconnected' | 'connecting';
  
  // Actions
  loadChats: () => Promise<void>;
  createChat: (counterparty: string) => Promise<void>;
  sendMessage: (chatId: string, content: string) => Promise<void>;
  updateSettings: (settings: Settings) => Promise<void>;
  connectWebSocket: () => void;
  searchChats: (query: string) => Promise<Chat[]>;
}
```

#### Step 2.3: WebSocket Integration

```typescript
// utils/websocket.ts
class ChatWebSocket {
  private ws: WebSocket | null;
  private reconnectTimer: NodeJS.Timeout | null;
  
  connect(onMessage: (msg: WsServerMessage) => void);
  send(message: WsClientMessage);
  disconnect();
  private handleReconnect();
  private sendHeartbeat();
}
```

#### Step 2.4: Mobile-First Styling

- Use CSS Grid and Flexbox for responsive layouts
- Touch-friendly button sizes (min 44x44px)
- Swipe gestures for navigation
- Bottom tab bar for easy thumb access
- Optimized for viewport height changes (keyboard)

### Phase 3: Public Browser Chat UI

#### Step 3.1: Public UI Structure (`/ui/public-chat/`)

1. **Join Flow**
   - Landing page at `/public/join-{uuid}`
   - Chat key input/generation
   - Cookie storage for chat key

2. **Chat Interface**
   - Simplified chat view
   - Limited to host communication
   - Download chat key option

#### Step 3.2: Authentication Flow

1. User visits public link
2. Checks for existing chat key cookie
3. If new user, generates chat key from host
4. Stores key as secure cookie
5. Authenticates WebSocket with key
6. Loads chat history

### Phase 4: Advanced Features

#### Step 4.1: Message Delivery Queue

1. **Queue Management**
   - Store failed messages in delivery_queue
   - Track delivery attempts and timestamps
   - Implement exponential backoff for retries

2. **Online Status Tracking**
   - Heartbeat mechanism
   - Status updates on connect/disconnect
   - Visual indicators in UI

#### Step 4.2: Full-Text Search

1. **Backend Search**
   - Index messages for efficient search
   - Search across all chats
   - Return highlighted results

2. **Frontend Search UI**
   - Search bar with debouncing
   - Results preview
   - Jump to message in chat

#### Step 4.3: File and Image Handling

1. **File Upload**
   - Use VFS for file storage
   - Generate preview thumbnails
   - Support multiple file types

2. **Image Display**
   - Conditional display based on settings
   - Lazy loading for performance
   - Pinch-to-zoom on mobile

### Phase 5: Testing and Polish

#### Step 5.1: Testing Strategy

1. **Unit Tests**
   - Message queue logic
   - State management
   - WebSocket handlers

2. **Integration Tests**
   - End-to-end chat flow
   - Browser chat authentication
   - Message delivery reliability

#### Step 5.2: Performance Optimization

1. **Backend**
   - Efficient message storage
   - Connection pooling
   - Lazy loading of chat history

2. **Frontend**
   - Virtual scrolling for long message lists
   - Image lazy loading
   - WebSocket reconnection logic

#### Step 5.3: Security

1. **Authentication**
   - Secure chat key generation
   - Token validation
   - Rate limiting

2. **Data Protection**
   - Input sanitization
   - XSS prevention
   - Secure cookie handling

## File Structure After Implementation

```
chat/
├── skeleton-app/           → chat-app/
│   ├── src/
│   │   └── lib.rs         # Core chat logic
├── ui/
│   ├── src/
│   │   ├── App.tsx        # Main app
│   │   ├── components/    # UI components
│   │   ├── store/         # Zustand stores
│   │   ├── types/         # TypeScript types
│   │   └── utils/         # Utilities
├── ui-public/             # Public browser chat UI
│   ├── src/
│   │   ├── App.tsx
│   │   └── ...
├── api/                   # Generated API bindings
└── pkg/
    └── manifest.json      # App manifest with capabilities
```

## Key Considerations

### Mobile-First Design
- Touch-friendly interfaces
- Responsive layouts
- Optimized for small screens
- Gesture support

### Real-Time Communication
- WebSocket for instant messaging
- Heartbeat for connection health
- Automatic reconnection
- Message queueing for reliability

### Data Persistence
- Use `OnDiff` save config for state changes
- Store chat history efficiently
- Handle large message volumes

### Security
- Authenticated node connections
- Secure browser chat keys
- Input validation and sanitization
- Rate limiting for API endpoints

### Extensibility
- Modular component structure
- Clear separation of concerns
- Prepared for Groups and Calls features
- Plugin-ready architecture

## Implementation Order

1. **Week 1**: Backend foundation (types, state, basic HTTP endpoints)
2. **Week 2**: WebSocket implementation and node-to-node chat
3. **Week 3**: Frontend UI (splash, chat list, basic chat view)
4. **Week 4**: Message delivery, queueing, and acknowledgments
5. **Week 5**: Browser chat support and public UI
6. **Week 6**: Advanced features (search, file upload, settings)
7. **Week 7**: Testing, optimization, and polish

## Notes for Implementor

- Start with the backend implementation in `skeleton-app/src/lib.rs`
- Build the app with `kit build --hyperapp` to generate API bindings
- Reference the voice example app in `resources/example-apps/voice/` for WebSocket patterns
- Use the generated `target/ui/caller-utils.ts` for all backend API calls
- Test node-to-node chat before implementing browser chat
- Ensure mobile responsiveness at every step
- Follow the existing code style and patterns in the skeleton app

Remember: The API is machine-generated from the Rust backend. Define types and functions in Rust first, then consume them in the frontend.