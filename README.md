# chat

The Official Hyperware Chat App: 1-1 Chats, multi-party Groups, and Calls.

![chat](https://raw.githubusercontent.com/hyperware-ai/chat/refs/heads/master/ui/public/chat-256.png)

## Build

```
kit build --hyperapp --features "caller-utils"
```

## Screenshots

![chat-list](https://raw.githubusercontent.com/hyperware-ai/chat/refs/heads/master/assets/chat-list.png)

![chat](https://raw.githubusercontent.com/hyperware-ai/chat/refs/heads/master/assets/chat.png)

## Tests

To run integration tests:
`kit run-tests`

To run unit tests at the end of files:
`cargo test -p chat --tests`

## Fast UI dev loop (fake node + Vite + Playwright)

1. Start a fake node (pick a unique home/port if 8080 is busy):  
   `kit boot-fake-node --home /tmp/hyperware-dev-node --port 8080 --fake-node-name devnode -- --detached`
2. Run the Vite dev server against that node (use Node 20):  
   `PATH=$HOME/.nvm/versions/node/v20.19.6/bin:$PATH VITE_NODE_URL=http://127.0.0.1:8080 npm run dev -- --host --port 4173`  
   (Vite will show the actual port if 4173 is taken; base URL looks like `http://localhost:4173/chat:chat:ware.hypr/`.)
3. Point the UI runner at the dev server and skip chat start:  
   `node codex_work/ui-runner.js --nodes 1 --dev-url http://localhost:4173/chat:chat:ware.hypr/ --skip-chat`

This lets you edit `ui/src/...`, see changes live via Vite, and validate the full group flow via Playwright without rebuilding the package each time.
