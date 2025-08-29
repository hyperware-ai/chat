# 250822

## The Official Chat App

A hyperapp that allows communication in three modes:
1. 1:1 DMs (Chats)
2. Group chats (Groups)
3. 1:1 voice calls (Calls)

Mobile-first, but also works on desktop.
All UI must be designed with mobile-first in mind.

The four main screens correspond to a splash screen and those 3 communication modes, discussed in the "Frontend" section

### Backend

Use a `#[ws]` connection to an authenticated endpoint in order to send and receive in realtime between our frontend and node.

#### Chat

Chats are in two modes: node-to-node and node-to-browser.

##### Node-to-node

Two nodes chat with each other.
Each maintains a full history of the chat.
When a node sends a message to a counterparty, it expects an ack.
If it does not receive an ack, it places the message in an "attempt delivery" queue.
It periodically attempts to deliver messages to nodes from this queue -- in the proper order.
It also notes that a node is offline, and if a node receives a message from a previously offline node it attempts delivery of all queued messages to that node immediately.

##### Node-to-browser

One node (the "host") creates a browser link.
It serves a public UI at that endpoint (i.e. `/public/join-<uuid>`).
It also serves a general public UI at, e.g. `/public` which is what a user with a chat key file cookie can chat through.
That link can either be a single use or perpetual link.
If single use, it allows issuance of one chat key file only; if perpetual, it allows issuance of arbitrarily many chat key files.
When another user (the "user") follows the link, they are prompted whether they want to start a chat with the node and prompts if the user has a chat key file.
The chat UI is mostly the same, but the user's name is controlled by the host and all data is stored on the host's machine.
The user's chat app pulls all data from the host's machine and pushes chat data to it.
The user is issued a chat key which is stored as a cookie that identifies the user.
The user can only chat with the host but also has the ability to download the chat key.
If the user is disconnected or joins a chat withthe host from another machine, the user can supply the chat key file and will assume the same identity and be able to view the full chat history.
In other words, the "chat key file" is like an API key for chatting with the host.
The host can revoke the chat key/kill the conversation.

#### Groups

TODO later

#### Calls

TODO later

### Frontend

#### Splash screen

Signal-esque screen with three tabs on bottom: Chats, Groups, Calls.
Top left has a circle that shows user profile pic.
If tapped, opens drop-down menu that has button to open Settings in it

##### Global settings

Allow setting:
- profile picture (default is a gray circle with black text: first letter of node name i.e. `foo.os` -> `f`)
- whether to show images in chats/groups (or just the link)
- whether to show user's profile pics (or just the default)
- whether to combine Chats & Groups into one tab `Chats & Groups`: this puts them all in one tab and makes the `New` button open a drop-down menu to choose `New Chat` vs `New Group` (default is chats & groups are separate)
- globally whether Chats should notify on receiving new message
- globally whether Groups should notify on receiving new message
- globally whether Calls should notify on ring
- whether to change voice notes into text with STT (and API key if yes)
- whether to allow node-to-browser chats

##### Chats

Show list of chats ordered by activity recency.
Show a `Search` bar up top that will do full-text search through all chats.
Top-right has `New Chat` button (icon, not text in button)

##### Groups

Show list of chats ordered by activity recency.
Show a `Search` bar up top that will do full-text search through all groups.
Top-right has `New Group` button (icon, not text in button)

##### Calls

Show history of calls (who, when, missed/duration, incoming vs outgoing)

#### Chat

Top-left shows a back button to splash screen (swiping from left also goes back to splash screen).
Top shows counterparty name and profile pic; tapping opens Chat-level settings.
Top-right shows a button to start a voice call.
Don't show participant names in the chat.
Show date inline with messages (i.e. `Yesterday` then a bunch of messages then `Today` then today's messages).
Swipe a message from counterparty to reply to that message.
Holding a message opens a menu: Reply; Forward; Copy; Select; Info; Delete; Edit (if our message); + emoji reacts.
Bottom bar has + on far left for uploading files or images.
Bottom bar has Message field for inputting chat message.
Bottom bar has a microphone for voice notes: record and send voice in chat OR use STT if set in settings.

When sending a message, show small text in the lower right of the message that indicate its delivery status:
- `...`: message has been sent from FE to our node but has not yet been received by our node
- `Sending`: message has been received by our node and is attempting to be sent to the other node
- `Receieved`: message has been received by the other node

##### Chat-level settings

- whether to notify on receiving new message
- whether to block communication

#### Group

TODO later

#### Call

TODO later
