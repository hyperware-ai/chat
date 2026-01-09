UI runner helper
================

Human readable
-----------

This repo is used to develop the chat app. 
ui-runner.js runs through a large ui flow, and at each point of the way makes screenshots to verify that the UI is behaving as expected. 
In that sense it behaves as a kind of Chat UI testing suite.
Codex, Opus and other agents can be asked to read this README and add a feature to the app.
When they follow the instructions carefully, they use the feedback loop to verify whether the new feature they added works.
This allows QA with multiple networked nodes and UI actions (see "Building new features" section).
Make sure to use `codex --yolo` or `claude --dangerously-skip-permissions` in order for the models to be able to freely boot nodes, install apps, and make changes.

The rest of this document is for AIs.

-----------

This folder contains a Playwright runner that drives the chat hyperapp, captures artifacts, and targets fake nodes you boot separately (see below).
Pay special attention to the "Building new features" section, and follow it step-by-step.

Quick start
-----------
1) Make sure Node 20 is available (nvm is already installed):
   - `env -u npm_config_prefix bash -lc "source ~/.nvm/nvm.sh && nvm use 20"` (installs if missing)
2) Install deps once: `npm install` (from this folder).
3) Assume a fake node has been booted by the user on port 8080
4) Run the basic loop (reuses that node, builds/starts chat UI via `kit bs --hyperapp --features caller-utils` each time, drives login/chat, captures artifacts):
   - `npm run ui:run -- --nodes 1 --reuse`
   - Flags:
     - `--headed` to see the browser.
     - `--skip-chat` to avoid auto-running `kit bs` (if you use it, start `kit bs --hyperapp --features caller-utils .` yourself; no `kit dev-ui`/`vite`).
     - `--reuse` required when pointing at pre-booted nodes.
     - `--base-port <port>` to change 8080/8081 defaults.
     - `--chat-path <path>` if the chat route changes (default `/chat:chat:ware.hypr/`).

What it does
------------
- Drives against N fake nodes you booted separately (`kit boot-fake-node -- ... --detached`) with homes under `/tmp/hyperware-fake-node-*`.
- Starts the chat package via `kit bs --hyperapp --features caller-utils .` (working dir `../chat`) against the node port unless `--skip-chat` is set. This is a fresh build/run each time (no `kit dev-ui`/`vite` reuse). Environment is sanitized to avoid the `npm_config_prefix`/nvm conflict.
- For each node: log in with a password (default `codex-test-pass`), navigate to the chat route, take a full-page screenshot, and dump DOM/state/console/network/failed-requests.
- Copies backend `.terminal_logs` into the run folder.

Building new features
---------
1. User will ask you for a feature
2. Build it in chat/ui/src
3. If neccessary, add a function in the runner which tests the feature (whether just clicking on it, or actually changing data, depending on what the feature is)
4. Run "kit bs --hyperapp --features caller-utils && kit s --port 8081" from chat, so that its installed into the node
5. Do a ui run and inspect carefully the new feature based on the artifacts(the screenshots and the jsons). View the screenshots and examine whether the change has been implemented correctly.
6. If the feature is not executed correctly, go back to step 2.
7. Once done - run `say "AI work done"`

Artifacts
---------
- Runs are stored under `chat_dev_loop/ui-runs/<timestamp>/`.
  - `metadata.json`: run config + per-node status.
  - `chat.log`: output from `kit bs`.
  - `node-<name>/screens/`: step-by-step navigation screenshots (every page/modal visit).
  - `node-<name>/terminal.log`: fake node log.
  - If the page loads: `dom.html`, `state.json`, `console.json`, `network.json`, `requests-failed.json`.
- After every `ui:run`, confirm the task is complete from the captured artifacts; rely on the `screens/` trail and metadata (group flow) for verification.
- Troubleshooting:
  - If a run appears hung or produces no output, check for stray `node ui-runner.js` processes (`ps -ef | grep ui-runner | grep -v grep`) and kill them before rerunning.
  - The runner navigates back to the chats list using back buttons and tab clicks; if it still lands in a DM view, inspect `node-*/screens/` to see the path and adjust selectors as needed.
  - Each time you take a screenshot, inspect it so you know exactly what it shows; use that to decide next navigation/steps (e.g., confirm the list view vs. an in-chat view before proceeding).
  - When adding a new feature, always produce a screenshot path that shows the feature working, verify it yourself, and only then report back.
  - `node-*/screens/` is the primary trail; if `chats-tab.png` or `screen.png` is stale, rely on the step-by-step shots to verify features.

Troubleshooting
---------------
- If `kit bs` prompts for Node/npm installs, ensure `nvm use 20` was run and `npm_config_prefix` is unset (the runner sets this to undefined when spawning).
- If chat navigation 404s, double-check the chat route or start command; rerun with `--skip-chat` and start the app manually:
  - `env -u npm_config_prefix bash -lc "source ~/.nvm/nvm.sh && nvm use 20 && cd ../chat && kit bs --hyperapp --features caller-utils ."`
- If login or chat keeps failing, inspect the latest `ui-runs/<run>/chat.log` and `node-*/terminal.log` to see whether the app started cleanly.
- Avoid hand-running `npm install`/`npm run build` inside `chat/ui`; always rely on `kit` (e.g., `kit run-tests` or the runner’s `kit bs`) to install and build with the correct platform-specific rollup bundle. If you need to rebuild locally, let `kit` drive it for you.
- Environment rule of thumb:
  - UI work: keep fake nodes running and drive via the UI runner (`npm run ui:run ...`); do **not** run `kit run-tests` because it tears down/assumes no running nodes.
  - Backend work: run `kit run-tests` as usual (nodes should not be pre-running for that flow).
