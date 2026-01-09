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

A lot of functions have the following comment above them in the app.
```Rust
// uncomment #[remote] for tests
// #[remote]
```
Testing suite requires them to be accessible as remote, so that the master testing node can call them.
So to run integration tests with multiple nodes:
- uncomment all the commented `#[remote]` functions
- `kit run-tests`
Make sure to "re-comment" only those `#[remote]` attributes which have `// uncomment #[remote] for tests` above them, as some `#[remote]` attributes should remain.

To run unit tests at the end of files:
`cargo test -p chat --tests`

`chat_dev_loop` can be used for integration testing with the UI.

