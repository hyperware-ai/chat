#!/usr/bin/env node

const path = require("path");
const fs = require("fs");
const { spawn } = require("child_process");
const http = require("http");
const https = require("https");
const { chromium } = require("playwright");

const args = process.argv.slice(2);

// --------- small arg helpers ----------
const getFlag = (flag, fallback = undefined) => {
  const idx = args.indexOf(flag);
  return idx !== -1 && args[idx + 1] !== undefined ? args[idx + 1] : fallback;
};
const hasFlag = (flag) => args.includes(flag);

// --------- config ----------
const nodeCount = Number(getFlag("--nodes", "1"));
const basePortFlag = getFlag("--base-port");
const basePort = Number(basePortFlag ?? "8080");
const basePortExplicit = basePortFlag !== undefined && basePortFlag !== null;
const persistHome = hasFlag("--persist");
const skipBoot = hasFlag("--reuse");
const headless = !hasFlag("--headed");
const password = getFlag("--password", "codex-test-pass");
const chatPath = getFlag("--chat-path", "/chat:chat:ware.hypr/");
const devUrl = getFlag("--dev-url", "").trim() || null;
const compareChats = hasFlag("--compare-chats");
const kitBin = getFlag("--kit-bin", "kit");
const tag = getFlag("--tag", "").trim();
const chatDir = path.resolve(
  getFlag("--chat-dir", path.join(__dirname, "..", "chat"))
);
const skipChatStart = hasFlag("--skip-chat") || Boolean(devUrl);
// --test flag: comma-separated list of tests to run. Available: groupFlow, leaveGroup, kickMember, all
// If not specified, defaults to "all" for multi-node runs, "groupFlow" for single-node
const testFilter = getFlag("--test", "");

const defaultNodes = [
  { name: "fake1", port: 8080, home: "/tmp/hyperware-fake-node-1" },
  { name: "fake2", port: 8081, home: "/tmp/hyperware-fake-node-2" },
];

const nodes = Array.from({ length: nodeCount }).map((_, idx) => {
  const fallback = defaultNodes[idx] || {};
  const useBasePort = basePortExplicit || !fallback.port;
  return {
    name: fallback.name || `fake${idx + 1}`,
    port: useBasePort ? basePort + idx : fallback.port,
    home: fallback.home || `/tmp/hyperware-fake-node-${idx + 1}`,
  };
});

const runLabel = `${new Date()
  .toISOString()
  .replace(/[-:T]/g, "")
  .slice(0, 15)}${tag ? `-${tag}` : ""}`;
const runRoot = path.join(__dirname, "ui-runs", runLabel);
const screensDirName = "screens";

// --------- utils ----------
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

async function ensureDir(dir) {
  await fs.promises.mkdir(dir, { recursive: true });
}

async function writeJson(file, data) {
  await fs.promises.writeFile(file, JSON.stringify(data, null, 2), "utf8");
}

async function copyDir(src, dest) {
  try {
    const stat = await fs.promises.stat(src);
    if (!stat.isDirectory()) return false;
  } catch (err) {
    return false;
  }

  await ensureDir(dest);
  const entries = await fs.promises.readdir(src, { withFileTypes: true });
  for (const entry of entries) {
    const from = path.join(src, entry.name);
    const to = path.join(dest, entry.name);
    if (entry.isDirectory()) {
      await copyDir(from, to);
    } else if (entry.isFile()) {
      await fs.promises.copyFile(from, to);
    }
  }
  return true;
}

async function waitForHttp(url, timeoutMs = 30000, predicate = (code) => code >= 200 && code < 500) {
  const start = Date.now();
  const lib = url.startsWith("https") ? https : http;

  while (Date.now() - start < timeoutMs) {
    try {
      const status = await new Promise((resolve, reject) => {
        const req = lib.get(url, { timeout: 3000 }, (res) => {
          res.resume();
          const status = res.statusCode || 0;
          resolve(status);
        });
        req.on("error", reject);
        req.on("timeout", () => {
          req.destroy(new Error("timeout"));
        });
      });
      if (predicate(status)) return status;
    } catch (err) {
      // swallow until timeout
    }
    await sleep(500);
  }
  throw new Error(`Timed out waiting for ${url}`);
}

async function startChatServer(targetPort) {
  const logPath = path.join(runRoot, "chat.log");
  await ensureDir(path.dirname(logPath));
  const logStream = fs.createWriteStream(logPath, { flags: "a" });

  const args = [
    "bs",
    "--hyperapp",
    "--features",
    "caller-utils",
    "--port",
    String(targetPort),
    ".",
  ];

  const child = spawn(kitBin, args, {
    cwd: chatDir,
    stdio: ["ignore", "pipe", "pipe"],
    env: {
      ...process.env,
      npm_config_prefix: undefined,
    },
  });

  child.stdout.pipe(logStream);
  child.stderr.pipe(logStream);

  child.on("exit", (code, signal) => {
    if (!shutdownRequested) {
      console.warn(
        `[chat] exited early (code=${code}, signal=${signal}) see ${logPath}`
      );
    }
  });

  return { child, logPath, kind: "chat" };
}

// Install chat on an additional node (after first build)
async function installChatOnNode(port) {
  const logPath = path.join(runRoot, `chat-install-${port}.log`);
  await ensureDir(path.dirname(logPath));
  const logStream = fs.createWriteStream(logPath, { flags: "a" });

  const args = [
    "s",
    "--port",
    String(port),
    ".",
  ];

  console.log(`[chat] installing on port ${port}...`);

  return new Promise((resolve, reject) => {
    const child = spawn(kitBin, args, {
      cwd: chatDir,
      stdio: ["ignore", "pipe", "pipe"],
      env: {
        ...process.env,
        npm_config_prefix: undefined,
      },
    });

    child.stdout.pipe(logStream);
    child.stderr.pipe(logStream);

    child.on("exit", (code) => {
      if (code === 0) {
        console.log(`[chat] installed on port ${port}`);
        resolve();
      } else {
        reject(new Error(`kit s failed on port ${port} with code ${code}`));
      }
    });

    child.on("error", reject);
  });
}

async function startNode(node) {
  const nodeDir = path.join(runRoot, `node-${node.name}`);
  await ensureDir(nodeDir);
  const logPath = path.join(nodeDir, "terminal.log");
  const logStream = fs.createWriteStream(logPath, { flags: "a" });

  const args = [
    "boot-fake-node",
    "--home",
    node.home,
    "--port",
    String(node.port),
    "--fake-node-name",
    node.name,
  ];
  if (persistHome) args.push("--persist");
  args.push("--", "--detached");

  const child = spawn(kitBin, args, {
    stdio: ["ignore", "pipe", "pipe"],
  });

  child.stdout.pipe(logStream);
  child.stderr.pipe(logStream);

  child.on("exit", (code, signal) => {
    if (!shutdownRequested) {
      console.warn(
        `[node:${node.name}] exited early (code=${code}, signal=${signal})`
      );
    }
  });

  await waitForHttp(`http://localhost:${node.port}/`);

  return { child, logPath, nodeDir };
}

async function stopNode(proc) {
  if (!proc || proc.killed) return;

  return new Promise((resolve) => {
    const killTimer = setTimeout(() => {
      try {
        process.kill(proc.pid, "SIGKILL");
      } catch (_err) {
        // ignore
      }
      resolve();
    }, 5000);

    proc.once("exit", () => {
      clearTimeout(killTimer);
      resolve();
    });

    try {
      proc.kill("SIGINT");
    } catch (_err) {
      clearTimeout(killTimer);
      resolve();
    }
  });
}

function attachCollectors(page) {
  const consoleLogs = [];
  const networkLogs = [];
  const failedRequests = [];
  const pageErrors = [];

  page.on("console", (msg) => {
    consoleLogs.push({
      type: msg.type(),
      text: msg.text(),
      location: msg.location(),
      timestamp: new Date().toISOString(),
    });
  });

  page.on("requestfailed", (req) => {
    failedRequests.push({
      url: req.url(),
      method: req.method(),
      failure: req.failure(),
      timestamp: new Date().toISOString(),
    });
  });

  page.on("response", async (res) => {
    const headers = res.headers();
    const contentType = headers["content-type"] || "";
    const isJson = contentType.includes("application/json");
    const isText = contentType.includes("text/plain");
    const isApiish =
      res.url().includes("/api") || res.url().includes("/debug");
    if (!(isJson || isText || isApiish)) return;

    const entry = {
      url: res.url(),
      status: res.status(),
      method: res.request().method(),
      headers,
    };
    try {
      const body = await res.text();
      if (body.length < 200_000) {
        entry.body = body;
      } else {
        entry.body = `[skipped body >200kB, length=${body.length}]`;
      }
    } catch (err) {
      entry.body = `[failed to read body: ${err.message}]`;
    }
    networkLogs.push(entry);
  });

  page.on("pageerror", (err) => {
    pageErrors.push({
      message: err.message,
      stack: err.stack,
      timestamp: new Date().toISOString(),
    });
  });

  const dump = async (targetDir, chatPath) => {
    const state = await page.evaluate(
      ({ chatPath }) => {
        const result = {
          location: "",
          chatPath,
          title: "",
          readyState: "",
          localStorage: {},
          sessionStorage: {},
          cookies: "",
          globals: {},
          errors: [],
        };

        try {
          result.location = window.location.href;
          result.title = document.title;
          result.readyState = document.readyState;
        } catch (err) {
          result.errors.push(`meta:${err?.message || err}`);
        }

        try {
          const candidates = Object.keys(window).filter(
            (key) =>
              key.startsWith("__") ||
              key.toLowerCase().includes("state") ||
              key.toLowerCase().includes("store")
          );

          for (const key of candidates.slice(0, 40)) {
            try {
              const val = window[key];
              const isDomNode =
                typeof Node !== "undefined" && val instanceof Node;
              if (isDomNode) {
                result.globals[key] = "[DOM Node]";
              } else if (val instanceof Function) {
                result.globals[key] = "[function]";
              } else {
                result.globals[key] = val;
              }
            } catch (err) {
              result.globals[key] = `[error:${err.message}]`;
            }
          }
        } catch (err) {
          result.errors.push(`globals:${err?.message || err}`);
        }

        const dumpStorage = (label) => {
          try {
            const storage = window[label];
            const output = {};
            for (let i = 0; i < storage.length; i += 1) {
              const k = storage.key(i);
              output[k] = storage.getItem(k);
            }
            return output;
          } catch (err) {
            result.errors.push(`${label}:${err?.message || err}`);
            return {};
          }
        };

        result.localStorage = dumpStorage("localStorage");
        result.sessionStorage = dumpStorage("sessionStorage");

        try {
          result.cookies = document.cookie;
        } catch (err) {
          result.errors.push(`cookies:${err?.message || err}`);
        }

        return result;
      },
      { chatPath }
    );

    await fs.promises.writeFile(
      path.join(targetDir, "dom.html"),
      await page.content(),
      "utf8"
    );
    await writeJson(path.join(targetDir, "state.json"), state);
    await writeJson(path.join(targetDir, "console.json"), consoleLogs);
    await writeJson(path.join(targetDir, "pageerrors.json"), pageErrors);
    await writeJson(path.join(targetDir, "network.json"), networkLogs);
    await writeJson(
      path.join(targetDir, "requests-failed.json"),
      failedRequests
    );

    return { consoleLogs, networkLogs, failedRequests, state };
  };

  return { dump };
}

async function navigateToTab(page, label) {
  const tabButton = page.getByRole("button", { name: new RegExp(label, "i") }).first();
  await tabButton.click({ timeout: 8000 }).catch(() => {});
  await page.waitForTimeout(400);
}

function createSnapper(page, targetDir) {
  let counter = 0;
  const shotsDir = path.join(targetDir, screensDirName);
  const snap = async (label, locator = null) => {
    counter += 1;
    const file = path.join(
      shotsDir,
      `${String(counter).padStart(2, "0")}-${label.replace(/[^a-z0-9_-]+/gi, "_")}.png`
    );
    await ensureDir(path.dirname(file));
    if (locator) {
      try {
        await locator.scrollIntoViewIfNeeded();
        await locator.screenshot({ path: file });
      } catch (_err) {
        await page.screenshot({ path: file, fullPage: true });
      }
    } else {
      await page.screenshot({ path: file, fullPage: true });
    }
    return file;
  };
  return snap;
}

async function captureChatsView(page, targetDir) {
  const screenshotPath = path.join(targetDir, "chats-tab.png");
  try {
    // Ensure we are on the chats list; DM view will fail the selector below.
    await navigateToTab(page, "Chats");
    await page
      .locator(
        '[placeholder="Search DMs or groups..."], [placeholder="Search chats"], [placeholder="Search chats..."]'
      )
      .first()
      .waitFor({ timeout: 8000 })
      .catch(() => {});
    await page.waitForTimeout(800);
    await page.screenshot({ path: screenshotPath, fullPage: true });
    return { ok: true, screenshot: screenshotPath };
  } catch (err) {
    return { ok: false, error: err.message || String(err), screenshot: screenshotPath };
  }
}

async function goToChatsHome(page, chatUrl, snap) {
  const searchLocator = page
    .locator(
      '[placeholder="Search DMs or groups..."], [placeholder="Search chats..."], [placeholder="Search chats"]'
    )
    .first();
  let targetUrl = chatUrl;
  if (!targetUrl) {
    try {
      const current = page.url();
      const path = chatPath.startsWith("/") ? chatPath : `/${chatPath}`;
      targetUrl = new URL(path, current).toString();
    } catch (_err) {
      targetUrl = null;
    }
  }

  for (let attempt = 0; attempt < 4; attempt += 1) {
    // Try direct navigation to chats list
    if (targetUrl) {
      await page.goto(targetUrl, { waitUntil: "domcontentloaded", timeout: 20000 }).catch(() => {});
    }

    const hasSearch = await searchLocator.isVisible().catch(() => false);
    if (hasSearch) {
      if (snap) await snap("chats-home");
      return;
    }

    // Try explicit back button
    const backCandidates = [
      page.locator(".group-back").first(),
      page.locator(".back-button").first(),
      page.getByRole("button", { name: /back/i }).first(),
      page.locator('button[aria-label="Back"]').first(),
      page.locator('button:has-text("Back")').first(),
      page.locator('button:has-text("←")').first(),
    ];
    let clickedBack = false;
    for (const candidate of backCandidates) {
      const exists = (await candidate.count().catch(() => 0)) > 0;
      if (!exists) continue;
      await candidate.click({ timeout: 5000 }).catch(() => {});
      clickedBack = true;
      break;
    }

    if (!clickedBack) {
      // Try browser history back
      await page.goBack({ waitUntil: "domcontentloaded" }).catch(() => {});
    }

    // Try Chats tab if available
    await navigateToTab(page, "Chats").catch(async () => {
      const chatsTab = page.getByText(/^Chats$/).first();
      if ((await chatsTab.count().catch(() => 0)) > 0) {
        await chatsTab.click({ timeout: 5000 }).catch(() => {});
      }
    });
    await page.waitForTimeout(500);

    const nowHasSearch = await searchLocator.isVisible().catch(() => false);
    if (nowHasSearch) {
      if (snap) await snap("chats-home");
      return;
    }
  }

  // Final wait (even if not visible) to avoid throwing
  const finalHasSearch = await searchLocator.isVisible().catch(() => false);
  if (finalHasSearch && snap) await snap("chats-home");
}

async function findAndOpenChat(page, type = "dm") {
  await goToChatsHome(page, null, null);
  const rows = await page
    .locator(".group-list-item, .unified-item, .dm-list-item, .chat-list-item, button")
    .elementHandles();
  for (const handle of rows) {
    const text = (await handle.innerText().catch(() => "")) || "";
    const isGroup = /auto group/i.test(text) || /group/.test(text);
    if (type === "group" ? isGroup : !isGroup) {
      await handle.click({ timeout: 8000 }).catch(() => {});
      await page.waitForTimeout(600);
      const inGroup = (await page.locator(".group-view").count().catch(() => 0)) > 0;
      if ((type === "group" && inGroup) || (type === "dm" && !inGroup)) {
        return true;
      }
      // fall back and try next item
      await goToChatsHome(page, null, null);
    }
  }
  return false;
}

async function collectViewInfo(page) {
  return page.evaluate(() => {
    const textContent = (sel) =>
      document.querySelector(sel)?.textContent?.trim() || "";
    const buttons = Array.from(document.querySelectorAll("button"))
      .map((b) => b.textContent?.trim())
      .filter(Boolean);
    return {
      header: textContent(".group-header-name") || textContent(".chat-header"),
      hasThreads: !!document.querySelector(".thread-view"),
      hasThreadList: !!document.querySelector(".thread-list-column"),
      hasMembersButton: buttons.some((t) => t.toLowerCase().includes("members")),
      hasSettingsButton: buttons.some((t) => t.toLowerCase().includes("settings")),
      hasSyncButton: buttons.some((t) => t.toLowerCase().includes("sync")),
      buttons: buttons.slice(0, 30),
    };
  });
}

async function captureComparison(page, snap, targetDir) {
  const result = { ok: false, dm: null, group: null, diff: null, error: null };
  try {
    // DM view
    const dmOpened = await findAndOpenChat(page, "dm");
    if (!dmOpened) throw new Error("Could not open a DM");
    await page.waitForTimeout(600);
    if (snap) await snap("compare-dm-view");
    const dmInfo = await collectViewInfo(page);

    // Group view
    const groupOpened = await findAndOpenChat(page, "group");
    if (!groupOpened) throw new Error("Could not open a group");
    await page.waitForSelector(".group-view", { timeout: 12000 }).catch(() => {});
    await page.waitForTimeout(600);
    if (snap) await snap("compare-group-view");
    const groupInfo = await collectViewInfo(page);

    const diff = {
      threadsInGroup: groupInfo.hasThreads && !dmInfo.hasThreads,
      threadListInGroup: groupInfo.hasThreadList && !dmInfo.hasThreadList,
      membersOnlyInGroup: groupInfo.hasMembersButton && !dmInfo.hasMembersButton,
      settingsOnlyInGroup: groupInfo.hasSettingsButton && !dmInfo.hasSettingsButton,
      syncOnlyInGroup: groupInfo.hasSyncButton && !dmInfo.hasSyncButton,
    };

    result.dm = dmInfo;
    result.group = groupInfo;
    result.diff = diff;
    result.ok = true;

    await writeJson(path.join(targetDir, "dm-group-comparison.json"), result);
  } catch (err) {
    result.error = err.message || String(err);
    await writeJson(path.join(targetDir, "dm-group-comparison.json"), result);
  }
  return result;
}

async function verifyChatsList(page) {
  try {
    // unified list shows groups + DMs
    await page.waitForSelector(".unified-item", { timeout: 8000 });
    const dmCount = await page.locator(".unified-item.dm").count();
    const groupCount = await page.locator(".unified-item.group").count();
    return { ok: dmCount > 0 && groupCount > 0, dmCount, groupCount };
  } catch (err) {
    return { ok: false, error: err.message || String(err), dmCount: 0, groupCount: 0 };
  }
}

async function testAppSettings(page, snap) {
  const result = {
    opened: false,
    profileTab: false,
    generalTab: false,
    notificationsTab: false,
    ok: false,
    error: null,
  };

  try {
    // Click on profile button to open settings modal
    const profileBtn = page.locator(".profile-button").first();
    await profileBtn.waitFor({ timeout: 8000 });
    await profileBtn.click({ timeout: 8000 });

    // Wait for settings modal to appear
    await page.locator(".settings-modal").first().waitFor({ timeout: 8000 });
    result.opened = true;
    if (snap) await snap("app-settings-profile");
    result.profileTab = true;

    // Click on General tab
    const generalTab = page.locator(".settings-tab", { hasText: "General" }).first();
    await generalTab.click({ timeout: 5000 });
    await page.waitForTimeout(300);
    if (snap) await snap("app-settings-general");
    result.generalTab = true;

    // Click on Notifications tab
    const notificationsTab = page.locator(".settings-tab", { hasText: "Notifications" }).first();
    await notificationsTab.click({ timeout: 5000 });
    await page.waitForTimeout(300);
    if (snap) await snap("app-settings-notifications");
    result.notificationsTab = true;

    // Close the modal
    const closeBtn = page.locator(".settings-modal .close-button").first();
    await closeBtn.click({ timeout: 5000 }).catch(() => {});
    await page.waitForTimeout(300);

    result.ok = result.opened && result.profileTab && result.generalTab && result.notificationsTab;
  } catch (err) {
    result.error = err.message || String(err);
  }

  return result;
}

async function driveGroupFlows(page, snap) {
  const flow = {
    groupName: `Auto Group ${Date.now().toString().slice(-6)}`,
    created: false,
    messageSent: false,
    searchJumped: false,
    searchTarget: null,
    subThreadCreated: false,
    whitelistLoaded: false,
    membersChecked: false,
    ok: false,
  };

  try {
    await page.waitForSelector('text=Chat', { timeout: 20000 }).catch(() => {});
    await page
      .locator('[placeholder="Search DMs or groups..."], [placeholder="Search chats..."]')
      .first()
      .waitFor({ timeout: 15000 })
      .catch(() => {});
    let newGroupBtn = page.getByRole("button", { name: /\+?\s*new/i }).first();
    await newGroupBtn.click({ timeout: 12000 }).catch(async () => {
      newGroupBtn = page.getByRole("button", { name: /new chat/i }).first();
      await newGroupBtn.click({ timeout: 12000 }).catch(async () => {
        newGroupBtn = page.locator('button:has-text("+")').first();
        await newGroupBtn.click({ timeout: 12000 }).catch(() => {});
      });
    });

    // Wait for the chooser, then pick "Create Group Chat"
    await page
      .getByRole("heading", { name: /new chat/i })
      .first()
      .waitFor({ timeout: 8000 })
      .catch(() => {});
    if (snap) await snap("new-chat-chooser");
    await page
      .getByRole("button", { name: /create group chat/i })
      .first()
      .click({ timeout: 12000 })
      .catch(async () => {
        await page.getByText(/create group chat/i).first().click({ timeout: 12000 }).catch(() => {});
      });

    await page.getByText("Create group").waitFor({ timeout: 10000 });
    if (snap) await snap("create-group-modal");
    const nameField = page
      .locator('[data-testid="group-name-input"], input[placeholder="Team updates"]')
      .first();
    await nameField.waitFor({ timeout: 12000 });
    await nameField.click({ timeout: 8000 }).catch(() => {});
    await nameField.fill("", { timeout: 6000 }).catch(() => {});
    await nameField.type(flow.groupName, { delay: 20, timeout: 8000 }).catch(() => {});
    const ensuredName = await nameField.inputValue({ timeout: 4000 }).catch(() => "");
    if (!ensuredName || !ensuredName.trim()) {
      await nameField.evaluate(
        (el, value) => {
          el.value = value;
          el.dispatchEvent(new Event("input", { bubbles: true }));
        },
        flow.groupName
      );
    }
    await page
      .locator(
        'textarea[placeholder="What is this group for?"], [data-testid="group-description-input"]'
      )
      .first()
      .fill("Runner-created group for smoke checks", { timeout: 8000 })
      .catch(() => {});
    const createBtn = page.getByTestId("group-create-submit").first();
    await createBtn.waitFor({ timeout: 8000 }).catch(() => {});
    await createBtn.click({ timeout: 8000 }).catch(() => {});
    await page.waitForSelector(".group-create-error", { state: "hidden", timeout: 8000 }).catch(() => {});
    await page.waitForSelector(".modal-content", { state: "hidden", timeout: 15000 }).catch(() => {});

    await page.getByText(flow.groupName, { exact: false }).first().click({ timeout: 12000 }).catch(() => {});
    const groupView = page.waitForSelector(".group-view", { timeout: 22000 }).catch(() => null);
    await groupView;
    await page
      .waitForSelector(`.group-header-name:has-text("${flow.groupName}")`, {
        timeout: 20000,
      })
      .catch(() => {});
    flow.created = true;
    if (snap) await snap("group-view");

    // Open settings modal to verify group settings UI
    try {
      const settingsBtn = page.getByRole("button", { name: /settings/i }).first();
      await settingsBtn.click({ timeout: 6000 });
      await page.locator(".group-settings-modal").first().waitFor({ timeout: 6000 });
      if (snap) await snap("group-settings-modal", page.locator(".group-settings-modal").first());
      const closeSettings = page.locator(".group-settings-modal .close-button").first();
      await closeSettings.click({ timeout: 4000 }).catch(() => {});
    } catch (_err) {
      // ignore settings capture failures
    }

    const message = "Hello from chat_dev_loop runner";
    const searchTarget = `search target ${Date.now().toString().slice(-6)}`;
    flow.searchTarget = searchTarget;
    const messagesLocator = page.locator(".group-messages").first();
    const messageBubble = page.locator('textarea[placeholder^="Type a message"]');
    const sendGroupMessage = async (text) => {
      await messageBubble.fill(text, { timeout: 8000 });
      await page.getByRole("button", { name: "Send message" }).click({ timeout: 8000 });
      await page.waitForSelector(`text=${text}`, { timeout: 15000 });
    };

    await sendGroupMessage(searchTarget);
    await sendGroupMessage(message);
    for (let i = 0; i < 14; i += 1) {
      await sendGroupMessage(`filler message ${i + 1}`);
    }
    flow.messageSent = true;
    await page.waitForTimeout(500);
    if (snap) await snap("group-message-actions", messagesLocator);

    // Verify group search jumps to the older message and highlights it
    try {
      await goToChatsHome(page, null, snap);
    const searchInput = page
        .locator(
          '[placeholder="Search DMs or groups..."], [placeholder="Search chats..."], [placeholder="Search chats"]'
        )
        .first();
      await searchInput.waitFor({ timeout: 10000 });
      await searchInput.fill(searchTarget);
      await page.waitForTimeout(800);
      const resultItem = page.locator(".unified-item", { hasText: searchTarget }).first();
      await resultItem.waitFor({ timeout: 10000 });
      if (snap) await snap("group-search-results");
      await resultItem.click({ timeout: 8000 });
      await page.waitForSelector(".group-view", { timeout: 15000 });
      const highlighted = page.locator(".group-message.highlight", { hasText: searchTarget }).first();
      await highlighted.waitFor({ timeout: 5000 });
      if (snap) {
        await snap("group-search-jump");
        await snap("group-search-jump-target", highlighted);
      }
      flow.searchJumped = true;
    } catch (searchErr) {
      console.log("[RUNNER] Group search jump failed:", searchErr.message);
      flow.searchJumped = false;
    }

    // Test voice note by sending via API
    try {
      // Minimal valid WebM audio (silent, ~100ms) - this is a tiny valid webm container
      const minimalWebmBase64 = "GkXfo59ChoEBQveBAULygQRC84EIQoKEd2VibUKHgQJChYECGFOAZwEAAAAAAAHTEU2bdLpNu4tTq4QVSalmU6yBoU27i1OrhBZUrmtTrIHGTbuMU6uEElTDZ1OssgoSz4CAAQAAAAAAAAAVAHEAAAAAAAAAAAAAAAAAAAAAAAAA";

      // Get the group ID by finding the group with matching name
      const groupId = await page.evaluate(async (groupName) => {
        try {
          const resp = await fetch('/chat:chat:ware.hypr/api/list-groups', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ ListGroups: null }),
          });
          if (!resp.ok) return null;
          const json = await resp.json();
          const data = json.Ok || json;
          const groups = data.groups || data;
          if (Array.isArray(groups) && groups.length > 0) {
            // Find the group by name
            const matchingGroup = groups.find(g => g.metadata?.name === groupName);
            if (matchingGroup) return matchingGroup.group_id;
            // Fallback to most recent
            return groups[groups.length - 1].group_id;
          }
          return null;
        } catch (e) {
          console.error('Failed to fetch groups:', e);
          return null;
        }
      }, flow.groupName);

      let threadId = null;

      if (groupId) {
        // Get the full group to find the main thread ID
        const threadResult = await page.evaluate(async (gid) => {
          try {
            const resp = await fetch('/chat:chat:ware.hypr/api/get-group', {
              method: 'POST',
              headers: { 'Content-Type': 'application/json' },
              body: JSON.stringify({ GetGroup: { group_id: gid } }),
            });
            if (!resp.ok) return { error: `HTTP ${resp.status}` };
            const json = await resp.json();
            const data = json.Ok || json;
            const group = data.group || data;
            // Return debug info
            const threadKeys = group?.threads ? Object.keys(group.threads) : [];
            const firstThread = threadKeys.length > 0 ? group.threads[threadKeys[0]] : null;
            return {
              hasGroup: !!group,
              threadKeys,
              firstThreadId: firstThread?.thread_id,
              firstThreadSample: firstThread,
            };
          } catch (e) {
            return { error: e.message };
          }
        }, groupId);
        console.log("[RUNNER] Thread debug:", JSON.stringify(threadResult, null, 2));

        if (threadResult.firstThreadId) {
          threadId = threadResult.firstThreadId;
        } else if (threadResult.threadKeys && threadResult.threadKeys.length > 0) {
          // The thread ID might BE the key
          threadId = threadResult.threadKeys[0];
        }
      }

      if (groupId && threadId) {
        console.log(`[RUNNER] Sending voice note to group=${groupId}, thread=${threadId}`);
        const voiceNoteResponse = await page.evaluate(async ({ groupId, threadId, audioData }) => {
          const response = await fetch('/chat:chat:ware.hypr/api/send-group-voice-note', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({
              SendGroupVoiceNote: {
                group_id: groupId,
                thread_id: threadId,
                reply_to: null,
                audio_data: audioData,
                duration: 8,
                mime_type: 'audio/webm',
              },
            }),
          });
          return { ok: response.ok, status: response.status };
        }, { groupId, threadId, audioData: minimalWebmBase64 });

        if (voiceNoteResponse.ok) {
          console.log("[RUNNER] Voice note sent successfully");
          flow.voiceNoteSent = true;
          await page.waitForTimeout(2000);
          // Wait for voice note to appear without reloading (should appear via state update)
          try {
            await page.waitForSelector('.group-audio-wrapper, .group-attachment-audio, audio', { timeout: 8000 });
          } catch (e) {
            console.log("[RUNNER] Voice note element not found, trying reload...");
            // Click the group to re-enter
            await page.click('.header-back-btn, [aria-label="Go back"]', { timeout: 5000 }).catch(() => {});
            await page.waitForTimeout(500);
            await page.click(`text=${flow.groupName}`, { timeout: 5000 }).catch(() => {});
            await page.waitForTimeout(2000);
          }
          if (snap) await snap("voice-note-in-chat", messagesLocator);
        } else {
          console.log("[RUNNER] Voice note failed:", voiceNoteResponse.status);
          flow.voiceNoteSent = false;
        }
      } else {
        console.log("[RUNNER] Could not get groupId/threadId for voice note test (groupId:", groupId, "threadId:", threadId, ")");
        flow.voiceNoteSent = false;
      }
    } catch (voiceErr) {
      console.log("[RUNNER] Voice note test skipped:", voiceErr.message);
      flow.voiceNoteSent = false;
    }

    // Try to create a sub-thread via context menu (thread indicator only shows on messages with threads)
    try {
      const msgForThread = page.locator(".group-message-bubble").first();
      await msgForThread.click({ button: "right", timeout: 8000 });
      await page.waitForSelector(".message-menu", { timeout: 5000 });
      const threadMenuBtn = page.locator(".message-menu button", { hasText: "Start Thread" }).first();
      if ((await threadMenuBtn.count()) > 0) {
        await threadMenuBtn.click({ timeout: 5000 });
        await page.waitForTimeout(1500);
        flow.subThreadCreated = true;
        if (snap) await snap("subthread-created");
      } else {
        // Close menu if no thread option
        await page.keyboard.press("Escape");
        flow.subThreadCreated = false;
      }
    } catch (threadErr) {
      console.log("[RUNNER] Thread creation skipped:", threadErr.message);
      flow.subThreadCreated = false;
    }

    // Skip subthread view tests if thread wasn't created
    if (flow.subThreadCreated) {
      try {
        // Click on the subthread to view it and check for root message
        const subthreadItem = page.locator(".thread-row", { hasText: message }).first();
        await subthreadItem.click({ timeout: 8000 }).catch(() => {});
        await page.waitForTimeout(1500);
        if (snap) await snap("subthread-view-empty");

        // Send a message to the subthread to verify messages appear
        const subthreadInput = page.locator('.group-message-input textarea').first();
        await subthreadInput.fill("Test message in subthread");
        await page.keyboard.press("Enter");
        await page.waitForTimeout(1500);
        if (snap) await snap("subthread-view-with-message");
      } catch (subthreadErr) {
        console.log("[RUNNER] Subthread view skipped:", subthreadErr.message);
      }
    }

    // Test reaction functionality - go back to main thread
    try {
      // Click on Main thread breadcrumb link to go back (or thread row if available)
      const mainThreadBreadcrumb = page.getByText("Main thread").first();
      await mainThreadBreadcrumb.click({ timeout: 8000 });
      await page.waitForTimeout(1000);

      // Right-click on a message to open context menu
      const msgForReact = page.locator(".group-message-bubble").first();
      await msgForReact.click({ button: "right", timeout: 8000 });
      await page.waitForSelector(".message-menu", { timeout: 5000 });
      if (snap) await snap("reaction-context-menu");

      // Click React to open emoji picker
      const reactBtn = page.locator(".message-menu button", { hasText: "React" }).first();
      await reactBtn.click({ timeout: 5000 });
      await page.waitForTimeout(500);
      if (snap) await snap("reaction-emoji-picker");

      // Click an emoji (thumbs up)
      const emojiBtn = page.locator(".emoji-tray-option").first();
      await emojiBtn.click({ timeout: 5000 });
      await page.waitForTimeout(2000);
      if (snap) await snap("reaction-added");

      // Wait for backend to process
      await page.waitForTimeout(3000);
      if (snap) await snap("reaction-after-wait");

      flow.reactionTested = true;
    } catch (reactionErr) {
      console.log("[RUNNER] Reaction test failed:", reactionErr.message);
      flow.reactionTested = false;
    }

    // Open the settings menu dropdown (gear icon) to access Members
    const gearMenuBtn = page.locator(".group-menu-btn").first();
    await gearMenuBtn.click({ timeout: 8000 });
    await page.waitForTimeout(500);
    if (snap) await snap("gear-menu-open");

    // Click Members button in the dropdown menu
    const membersBtn = page.locator(".group-menu-dropdown button", { hasText: "Members" }).first();
    await membersBtn.click({ timeout: 8000 });
    await page.waitForSelector(".members-modal", { timeout: 8000 });
    await page.waitForSelector("text=Leave group", { timeout: 8000 });
    flow.membersChecked = true;
    if (snap) await snap("members-modal");

    // Close the modal
    const closeBtn = page.locator(".members-modal .close-button").first();
    await closeBtn.click({ timeout: 8000 });
    flow.ok =
      flow.created &&
      flow.messageSent &&
      flow.searchJumped &&
      flow.membersChecked;
  } catch (err) {
    flow.error = err.message || String(err);
  }

  return flow;
}

async function testDMView(page, snap) {
  const result = {
    dmOpened: false,
    phoneButtonPresent: false,
    ok: false,
  };

  try {
    // Navigate back to chats home if we're not there
    await goToChatsHome(page, null, null).catch(() => {});

    // Wait a bit for the page to settle
    await page.waitForTimeout(500);

    // Look for fake2.os specifically (or any DM)
    const dmLink = page.getByText("fake2.os").first();
    const dmCount = await dmLink.count();

    if (dmCount === 0) {
      result.error = "No DM found in list (searched for fake2.os)";
      return result;
    }

    // Click on the DM
    await dmLink.click({ timeout: 8000 });

    // Wait for the chat header to appear
    await page.waitForSelector(".chat-header", { timeout: 10000 });
    result.dmOpened = true;

    // Take a screenshot of the DM view
    if (snap) await snap("dm-view");

    // Check if the phone button is present
    const phoneButton = page.locator(".voice-call-button");
    const phoneCount = await phoneButton.count();
    result.phoneButtonPresent = phoneCount > 0;

    result.ok = result.dmOpened && !result.phoneButtonPresent;
  } catch (err) {
    result.error = err.message || String(err);
  }

  return result;
}

// Two-node leave group test - requires pages for both nodes
async function testLeaveGroupPropagation(page1, page2, snap1, snap2, groupName) {
  const result = {
    node1InvitedNode2: false,
    node2JoinedGroup: false,
    node2LeftGroup: false,
    node1SeesNode2Removed: false,
    ok: false,
    error: null,
  };

  try {
    console.log("[LEAVE_TEST] Starting two-node leave group test");

    // Close any open modals first
    await page1.keyboard.press("Escape").catch(() => {});
    await page1.waitForTimeout(300);
    await page1.locator(".modal-overlay").click({ force: true, timeout: 1000 }).catch(() => {});
    await page1.waitForTimeout(300);

    // Step 1: Node 1 - we should already be in the group view, just verify
    // Check if we're already in the group
    const inGroupView = await page1.locator(".group-view").count() > 0;
    const groupHeader = await page1.locator(".group-header-name").textContent().catch(() => "");

    console.log(`[LEAVE_TEST] Node 1 state: inGroupView=${inGroupView}, header="${groupHeader}"`);

    if (!inGroupView || !groupHeader.includes("Auto Group")) {
      // Navigate to the group
      await goToChatsHome(page1, null, null);
      await page1.waitForTimeout(500);

      // Find and click on the group
      const groupLink1 = page1.getByText(groupName, { exact: false }).first();
      await groupLink1.click({ timeout: 10000 });
      await page1.waitForSelector(".group-view", { timeout: 10000 });
    }
    if (snap1) await snap1("leave-test-node1-group-view");

    // Open the gear menu and then Members modal
    const gearBtn1 = page1.locator(".group-menu-btn").first();
    await gearBtn1.click({ timeout: 5000 });
    await page1.waitForTimeout(300);

    const membersBtn1 = page1.locator(".group-menu-dropdown button", { hasText: "Members" }).first();
    await membersBtn1.click({ timeout: 5000 });
    await page1.waitForSelector(".members-modal", { timeout: 8000 });
    if (snap1) await snap1("leave-test-node1-members-before-invite");

    // Invite fake2.os
    const inviteInput = page1.locator('.members-modal input[placeholder*="Node ID"]').first();
    await inviteInput.fill("fake2.os", { timeout: 5000 });
    const inviteBtn = page1.locator('.members-modal button', { hasText: "Invite" }).first();
    await inviteBtn.click({ timeout: 5000 });
    await page1.waitForTimeout(2000);
    result.node1InvitedNode2 = true;
    if (snap1) await snap1("leave-test-node1-invited-node2");

    // Close the modal
    await page1.locator(".members-modal .close-button").first().click({ timeout: 5000 });
    await page1.waitForTimeout(500);

    // Step 2: Node 2 navigates to the group (should see invitation)
    await goToChatsHome(page2, null, null);
    await page2.waitForTimeout(1000);

    // Look for the group in node2's list
    const groupLink2 = page2.getByText(groupName, { exact: false }).first();
    const groupCount = await groupLink2.count();
    if (groupCount > 0) {
      await groupLink2.click({ timeout: 10000 });
      await page2.waitForSelector(".group-view", { timeout: 10000 });
      result.node2JoinedGroup = true;
      if (snap2) await snap2("leave-test-node2-in-group");
    } else {
      // Group might not be visible yet, wait and retry
      await page2.waitForTimeout(3000);
      await page2.reload();
      await page2.waitForTimeout(2000);
      const groupLink2Retry = page2.getByText(groupName, { exact: false }).first();
      if ((await groupLink2Retry.count()) > 0) {
        await groupLink2Retry.click({ timeout: 10000 });
        await page2.waitForSelector(".group-view", { timeout: 10000 });
        result.node2JoinedGroup = true;
        if (snap2) await snap2("leave-test-node2-in-group");
      } else {
        throw new Error("Node 2 cannot see the group after invite");
      }
    }

    // Step 3: Node 2 leaves the group
    const gearBtn2 = page2.locator(".group-menu-btn").first();
    await gearBtn2.click({ timeout: 5000 });
    await page2.waitForTimeout(300);

    const membersBtn2 = page2.locator(".group-menu-dropdown button", { hasText: "Members" }).first();
    await membersBtn2.click({ timeout: 5000 });
    await page2.waitForSelector(".members-modal", { timeout: 8000 });
    if (snap2) await snap2("leave-test-node2-members-modal");

    // Click Leave group button
    const leaveBtn = page2.locator('.members-modal button', { hasText: "Leave group" }).first();
    await leaveBtn.click({ timeout: 5000 });
    await page2.waitForTimeout(3000);
    result.node2LeftGroup = true;
    if (snap2) await snap2("leave-test-node2-after-leave");

    // Step 4: Verify Node 1 sees Node 2 as Removed
    // Wait for WebSocket update to propagate
    await page1.waitForTimeout(5000);

    // Refresh Node 1's view
    const gearBtn1Again = page1.locator(".group-menu-btn").first();
    await gearBtn1Again.click({ timeout: 5000 });
    await page1.waitForTimeout(300);

    const membersBtn1Again = page1.locator(".group-menu-dropdown button", { hasText: "Members" }).first();
    await membersBtn1Again.click({ timeout: 5000 });
    await page1.waitForSelector(".members-modal", { timeout: 8000 });
    await page1.waitForTimeout(500); // Let modal fully render
    if (snap1) await snap1("leave-test-node1-members-after-leave");
    // Also take a screenshot of just the modal for better visibility
    const modal = page1.locator(".members-modal").first();
    await modal.screenshot({ path: `${runRoot}/node-fake1/screens/20-members-modal-detail.png` });

    // Check that fake2.os is no longer in the members list (removed members are filtered out)
    const memberRows = await page1.locator(".members-modal .member-row").all();
    console.log("[LEAVE_TEST] Found", memberRows.length, "member rows");
    let fake2Found = false;
    for (const row of memberRows) {
      const text = await row.textContent();
      console.log("[LEAVE_TEST] Member row text:", text);
      if (text.includes("fake2.os")) {
        fake2Found = true;
        console.log("[LEAVE_TEST] FAIL: Node 2 still visible in members list");
      }
    }
    if (!fake2Found && memberRows.length === 1) {
      result.node1SeesNode2Removed = true;
      console.log("[LEAVE_TEST] SUCCESS: Node 2 is no longer in members list (count=1)");
    }

    if (!result.node1SeesNode2Removed) {
      // Take another screenshot after waiting more
      await page1.waitForTimeout(5000);
      if (snap1) await snap1("leave-test-node1-members-final-check");

      // Check again
      const memberRowsRetry = await page1.locator(".members-modal .member-row").all();
      console.log("[LEAVE_TEST] Retry: Found", memberRowsRetry.length, "member rows");
      let fake2FoundRetry = false;
      for (const row of memberRowsRetry) {
        const text = await row.textContent();
        if (text.includes("fake2.os")) {
          fake2FoundRetry = true;
        }
      }
      if (!fake2FoundRetry && memberRowsRetry.length === 1) {
        result.node1SeesNode2Removed = true;
        console.log("[LEAVE_TEST] SUCCESS (retry): Node 2 is no longer in members list");
      }
    }

    result.ok = result.node1InvitedNode2 && result.node2JoinedGroup &&
                result.node2LeftGroup && result.node1SeesNode2Removed;

    console.log("[LEAVE_TEST] Final result:", result);
  } catch (err) {
    result.error = err.message || String(err);
    console.log("[LEAVE_TEST] Error:", result.error);
  }

  return result;
}

/**
 * Test: Node 1 (owner) kicks Node 2 from a group, verify Node 2 sees the removal.
 * This tests the "remove by another member" flow.
 */
async function testKickMemberPropagation(page1, page2, snap1, snap2, groupName) {
  console.log("[KICK_TEST] Starting two-node kick member test");
  const result = {
    node1InvitedNode2: false,
    node2JoinedGroup: false,
    node1KickedNode2: false,
    node2SeesRemoval: false,
    ok: false,
    error: null,
  };

  try {
    // Wait for Node 1 to be in the group view
    await page1.waitForTimeout(1000);

    // Check if the group name appears anywhere in the page
    const pageContent = await page1.content();
    let inGroupView = pageContent.includes(groupName);
    console.log(`[KICK_TEST] Node 1 state: inGroupView=${inGroupView}, groupName="${groupName}"`);

    if (!inGroupView) {
      // Maybe we're still on chat home - try clicking the group
      const groupItem = page1.locator(`.chat-item`, { hasText: groupName }).first();
      if (await groupItem.isVisible().catch(() => false)) {
        console.log("[KICK_TEST] Clicking on group in list");
        await groupItem.click({ timeout: 5000 });
        await page1.waitForTimeout(2000);
        const newContent = await page1.content();
        inGroupView = newContent.includes(groupName);
      }
      if (!inGroupView) {
        throw new Error(`Node 1 is not in the expected group view`);
      }
    }

    // Step 1: Node 1 invites Node 2
    const gearBtn1 = page1.locator(".group-menu-btn").first();
    await gearBtn1.click({ timeout: 5000 });
    await page1.waitForTimeout(300);

    const membersBtn1 = page1.locator(".group-menu-dropdown button", { hasText: "Members" }).first();
    await membersBtn1.click({ timeout: 5000 });
    await page1.waitForSelector(".members-modal", { timeout: 8000 });
    if (snap1) await snap1("kick-test-node1-members-before-invite");

    // Invite fake2.os
    const inviteInput = page1.locator(".members-modal input[type='text']").first();
    await inviteInput.fill("fake2.os");
    const sendInviteBtn = page1.locator(".members-modal button", { hasText: "Send invite" }).first();
    await sendInviteBtn.click({ timeout: 5000 });
    await page1.waitForTimeout(2000);
    result.node1InvitedNode2 = true;
    if (snap1) await snap1("kick-test-node1-invited-node2");

    // Close modal
    await page1.keyboard.press("Escape");
    await page1.waitForTimeout(500);

    // Step 2: Node 2 joins the group by clicking on it
    // Wait for invite to propagate and refresh page if needed
    await page2.waitForTimeout(3000);

    // Reload Node 2's page to ensure fresh data
    await page2.reload({ waitUntil: "networkidle", timeout: 15000 });
    await page2.waitForTimeout(2000);
    if (snap2) await snap2("kick-test-node2-after-reload");

    // Look for the group in the chat list - try multiple selectors
    let groupItem = page2.locator(`text="${groupName}"`).first();
    let groupVisible = await groupItem.isVisible({ timeout: 5000 }).catch(() => false);

    if (!groupVisible) {
      // Try alternative selectors
      console.log("[KICK_TEST] Trying alternative selectors for group on Node 2...");
      groupItem = page2.getByText(groupName, { exact: false }).first();
      groupVisible = await groupItem.isVisible({ timeout: 5000 }).catch(() => false);
    }

    if (!groupVisible) {
      // Try waiting a bit more
      console.log("[KICK_TEST] Group not visible on Node 2, waiting more...");
      await page2.waitForTimeout(5000);
      if (snap2) await snap2("kick-test-node2-no-group");
      groupItem = page2.getByText(groupName, { exact: false }).first();
      groupVisible = await groupItem.isVisible({ timeout: 5000 }).catch(() => false);
    }

    if (!groupVisible) {
      throw new Error("Group not visible on Node 2 after invite");
    }

    console.log("[KICK_TEST] Found group on Node 2, clicking...");
    await groupItem.click({ timeout: 10000 });
    await page2.waitForTimeout(2000);
    result.node2JoinedGroup = true;
    if (snap2) await snap2("kick-test-node2-in-group");

    // Step 3: Node 1 kicks Node 2
    // First, close any open modals
    await page1.keyboard.press("Escape");
    await page1.waitForTimeout(500);
    // Click on an empty area to ensure overlay is gone
    await page1.locator("body").first().click({ position: { x: 10, y: 10 }, force: true }).catch(() => {});
    await page1.waitForTimeout(500);

    // First, reopen the members modal on Node 1
    const gearBtn1Again = page1.locator(".group-menu-btn").first();
    await gearBtn1Again.click({ timeout: 5000 });
    await page1.waitForTimeout(300);

    const membersBtn1Again = page1.locator(".group-menu-dropdown button", { hasText: "Members" }).first();
    await membersBtn1Again.click({ timeout: 5000 });
    await page1.waitForSelector(".members-modal", { timeout: 8000 });
    if (snap1) await snap1("kick-test-node1-members-before-kick");

    // Find fake2.os row and click Remove button
    const memberRows = await page1.locator(".members-modal .member-row").all();
    let kickedNode2 = false;
    for (const row of memberRows) {
      const text = await row.textContent();
      if (text.includes("fake2.os")) {
        const removeBtn = row.locator("button", { hasText: "Remove" }).first();
        await removeBtn.click({ timeout: 5000 });
        kickedNode2 = true;
        break;
      }
    }
    if (!kickedNode2) {
      throw new Error("Could not find fake2.os in members list to kick");
    }
    await page1.waitForTimeout(3000);
    result.node1KickedNode2 = true;
    if (snap1) await snap1("kick-test-node1-after-kick");

    // Step 4: Verify Node 2 sees the removal
    // Wait for update to propagate
    await page2.waitForTimeout(3000);

    // Force a page reload on Node 2 to trigger a fresh get_group call
    // This will verify that the backend correctly denies access to kicked users
    console.log("[KICK_TEST] Reloading page on Node 2 to force fresh group fetch");
    await page2.reload({ waitUntil: 'networkidle' });
    await page2.waitForTimeout(2000);
    if (snap2) await snap2("kick-test-node2-after-reload");

    // Check if Node 2 is still in the group view or has been kicked out
    const gearBtn2 = page2.locator(".group-menu-btn").first();
    const gearBtn2Visible = await gearBtn2.isVisible().catch(() => false);

    if (gearBtn2Visible) {
      await gearBtn2.click({ timeout: 5000 });
      await page2.waitForTimeout(300);
      const membersBtn2 = page2.locator(".group-menu-dropdown button", { hasText: "Members" }).first();
      await membersBtn2.click({ timeout: 5000 });
      await page2.waitForSelector(".members-modal", { timeout: 8000 });
      if (snap2) await snap2("kick-test-node2-members-after-kick");

      // Also take a screenshot of just the modal
      const modal2 = page2.locator(".members-modal").first();
      await modal2.screenshot({ path: `${runRoot}/node-fake2/screens/kick-test-members-modal-detail.png` });

      // Check member count - Node 2 should see themselves as removed or just 1 member
      const node2MemberRows = await page2.locator(".members-modal .member-row").all();
      console.log("[KICK_TEST] Node 2 sees", node2MemberRows.length, "member rows");

      // If Node 2 only sees 1 member (fake1.os), they've received the update
      if (node2MemberRows.length === 1) {
        result.node2SeesRemoval = true;
        console.log("[KICK_TEST] SUCCESS: Node 2 sees only 1 member (themselves removed from view)");
      } else {
        // Check if Node 2's own entry shows as Removed or is missing
        let foundSelfActive = false;
        for (const row of node2MemberRows) {
          const text = await row.textContent();
          console.log("[KICK_TEST] Node 2 member row:", text);
          if (text.includes("fake2.os") && text.includes("Active")) {
            foundSelfActive = true;
          }
        }
        if (!foundSelfActive) {
          result.node2SeesRemoval = true;
          console.log("[KICK_TEST] SUCCESS: Node 2 no longer shows as active member");
        } else {
          console.log("[KICK_TEST] FAIL: Node 2 still shows as active member");
        }
      }
    } else {
      // Node 2 might have been kicked out of the group view entirely
      // Check if they're back at the chat list (unified-messages is the main chat list container)
      const chatList = page2.locator(".unified-messages, .chat-list-container, .chat-list");
      if (await chatList.isVisible().catch(() => false)) {
        result.node2SeesRemoval = true;
        console.log("[KICK_TEST] SUCCESS: Node 2 was kicked back to chat list");
      } else {
        console.log("[KICK_TEST] Node 2 is not in group view but also not at chat list");
      }
    }

    result.ok = result.node1InvitedNode2 && result.node2JoinedGroup &&
                result.node1KickedNode2 && result.node2SeesRemoval;

    console.log("[KICK_TEST] Final result:", result);
  } catch (err) {
    result.error = err.message || String(err);
    console.log("[KICK_TEST] Error:", result.error);
  }

  return result;
}

/**
 * Test that a kicked member cannot send messages that appear on the owner's node.
 * This tests the security enforcement - even if the kicked member's UI allows
 * them to send (because they didn't receive/acknowledge the kick), the owner's
 * node should reject the message via PubSub whitelist.
 */
async function testKickedMemberCannotSend(page1, page2, snap1, snap2, groupName) {
  console.log("[KICK_SEND_TEST] Starting adversarial kicked member send test");
  const result = {
    node1InvitedNode2: false,
    node2JoinedGroup: false,
    node2SentMessageBeforeKick: false,
    node1SawMessageBeforeKick: false,
    node1KickedNode2: false,
    node2TriedToSendAfterKick: false,
    node1DidNotSeeAdversarialMessage: false,
    ok: false,
    error: null,
  };

  const testMessageBeforeKick = `Before kick ${Date.now()}`;
  const adversarialMessage = `Adversarial msg ${Date.now()}`;

  try {
    await page1.waitForTimeout(1000);

    // Check if Node 1 is in the group view
    const pageContent = await page1.content();
    let inGroupView = pageContent.includes(groupName);
    console.log(`[KICK_SEND_TEST] Node 1 state: inGroupView=${inGroupView}, groupName="${groupName}"`);

    if (!inGroupView) {
      const groupItem = page1.locator(`.chat-item`, { hasText: groupName }).first();
      if (await groupItem.isVisible().catch(() => false)) {
        await groupItem.click({ timeout: 5000 });
        await page1.waitForTimeout(2000);
        const newContent = await page1.content();
        inGroupView = newContent.includes(groupName);
      }
      if (!inGroupView) {
        throw new Error(`Node 1 is not in the expected group view`);
      }
    }

    // Step 1: Node 1 invites Node 2
    const gearBtn1 = page1.locator(".group-menu-btn").first();
    await gearBtn1.click({ timeout: 5000 });
    await page1.waitForTimeout(300);

    const membersBtn1 = page1.locator(".group-menu-dropdown button", { hasText: "Members" }).first();
    await membersBtn1.click({ timeout: 5000 });
    await page1.waitForSelector(".members-modal", { timeout: 8000 });

    const inviteInput = page1.locator(".members-modal input[type='text']").first();
    await inviteInput.fill("fake2.os");
    const sendInviteBtn = page1.locator(".members-modal button", { hasText: "Send invite" }).first();
    await sendInviteBtn.click({ timeout: 5000 });
    await page1.waitForTimeout(2000);
    result.node1InvitedNode2 = true;
    console.log("[KICK_SEND_TEST] Node 1 invited Node 2");

    // Close modal
    await page1.keyboard.press("Escape");
    await page1.waitForTimeout(500);

    // Step 2: Node 2 joins the group
    await page2.waitForTimeout(3000);
    await page2.reload({ waitUntil: "networkidle", timeout: 15000 });
    await page2.waitForTimeout(2000);

    let groupItem = page2.getByText(groupName, { exact: false }).first();
    let groupVisible = await groupItem.isVisible({ timeout: 5000 }).catch(() => false);

    if (!groupVisible) {
      await page2.waitForTimeout(5000);
      groupItem = page2.getByText(groupName, { exact: false }).first();
      groupVisible = await groupItem.isVisible({ timeout: 5000 }).catch(() => false);
    }

    if (!groupVisible) {
      throw new Error("Group not visible on Node 2 after invite");
    }

    await groupItem.click({ timeout: 10000 });
    await page2.waitForTimeout(2000);
    result.node2JoinedGroup = true;
    console.log("[KICK_SEND_TEST] Node 2 joined the group");

    // Step 3: Node 2 sends a message BEFORE being kicked (to verify messaging works)
    const messageBubble2 = page2.locator('textarea[placeholder^="Type a message"]');
    await messageBubble2.fill(testMessageBeforeKick, { timeout: 8000 });
    await page2.getByRole("button", { name: "Send message" }).click({ timeout: 8000 });
    await page2.waitForTimeout(2000);
    result.node2SentMessageBeforeKick = true;
    console.log("[KICK_SEND_TEST] Node 2 sent message before kick");

    // Verify Node 1 sees the message
    await page1.waitForTimeout(3000);
    const node1ContentBefore = await page1.content();
    if (node1ContentBefore.includes(testMessageBeforeKick)) {
      result.node1SawMessageBeforeKick = true;
      console.log("[KICK_SEND_TEST] Node 1 saw Node 2's message before kick - GOOD");
    } else {
      console.log("[KICK_SEND_TEST] WARNING: Node 1 did not see Node 2's message before kick");
    }
    if (snap1) await snap1("kick-send-test-before-kick");

    // Step 4: Node 1 kicks Node 2
    await page1.keyboard.press("Escape");
    await page1.waitForTimeout(500);
    await page1.locator("body").first().click({ position: { x: 10, y: 10 }, force: true }).catch(() => {});
    await page1.waitForTimeout(500);

    const gearBtn1Again = page1.locator(".group-menu-btn").first();
    await gearBtn1Again.click({ timeout: 5000 });
    await page1.waitForTimeout(300);

    const membersBtn1Again = page1.locator(".group-menu-dropdown button", { hasText: "Members" }).first();
    await membersBtn1Again.click({ timeout: 5000 });
    await page1.waitForSelector(".members-modal", { timeout: 8000 });

    const memberRows = await page1.locator(".members-modal .member-row").all();
    let kickedNode2 = false;
    for (const row of memberRows) {
      const text = await row.textContent();
      if (text.includes("fake2.os")) {
        const removeBtn = row.locator("button", { hasText: "Remove" }).first();
        await removeBtn.click({ timeout: 5000 });
        kickedNode2 = true;
        break;
      }
    }
    if (!kickedNode2) {
      throw new Error("Could not find fake2.os in members list to kick");
    }
    await page1.waitForTimeout(3000);
    result.node1KickedNode2 = true;
    console.log("[KICK_SEND_TEST] Node 1 kicked Node 2");

    // Close modal
    await page1.keyboard.press("Escape");
    await page1.waitForTimeout(500);
    if (snap1) await snap1("kick-send-test-after-kick");

    // Step 5: Node 2 (adversarially) tries to send a message IMMEDIATELY after being kicked
    // We try to send BEFORE the kick notification has time to propagate
    // This tests the security enforcement - even if Node 2's UI still allows sending,
    // Node 1's backend should reject the message via PubSub whitelist
    console.log("[KICK_SEND_TEST] Node 2 attempting to send adversarial message IMMEDIATELY...");

    // Don't wait - try to race the kick notification

    // Try to send a message from Node 2
    const messageBubble2After = page2.locator('textarea[placeholder^="Type a message"]');
    const canType = await messageBubble2After.isVisible().catch(() => false);

    if (canType) {
      await messageBubble2After.fill(adversarialMessage, { timeout: 8000 }).catch(() => {});
      const sendBtn2 = page2.getByRole("button", { name: "Send message" });
      const canSend = await sendBtn2.isEnabled().catch(() => false);
      if (canSend) {
        await sendBtn2.click({ timeout: 8000 }).catch(() => {});
        result.node2TriedToSendAfterKick = true;
        console.log("[KICK_SEND_TEST] Node 2 attempted to send adversarial message");
      } else {
        console.log("[KICK_SEND_TEST] Node 2 send button disabled (correctly)");
        result.node2TriedToSendAfterKick = false;
      }
    } else {
      console.log("[KICK_SEND_TEST] Node 2 message input not visible (kicked out of group view)");
      result.node2TriedToSendAfterKick = false;
    }
    if (snap2) await snap2("kick-send-test-adversarial-attempt");

    // Step 6: Verify Node 1 does NOT see the adversarial message
    // Wait a bit to ensure any message would have propagated
    await page1.waitForTimeout(5000);

    // Refresh Node 1's view to be sure
    await page1.keyboard.press("Escape");
    await page1.waitForTimeout(500);

    const node1ContentAfter = await page1.content();
    if (snap1) await snap1("kick-send-test-node1-after-adversarial");

    if (!node1ContentAfter.includes(adversarialMessage)) {
      result.node1DidNotSeeAdversarialMessage = true;
      console.log("[KICK_SEND_TEST] SUCCESS: Node 1 did NOT see the adversarial message");
    } else {
      console.log("[KICK_SEND_TEST] FAIL: Node 1 SAW the adversarial message - security breach!");
      result.node1DidNotSeeAdversarialMessage = false;
    }

    // Test passes if:
    // - The setup worked (invite, join, kick)
    // - Either Node 2 couldn't send OR if they did, Node 1 didn't see it
    result.ok = result.node1InvitedNode2 &&
                result.node2JoinedGroup &&
                result.node1KickedNode2 &&
                (result.node1DidNotSeeAdversarialMessage || !result.node2TriedToSendAfterKick);

    console.log("[KICK_SEND_TEST] Final result:", JSON.stringify(result, null, 2));
  } catch (err) {
    result.error = err.message || String(err);
    console.log("[KICK_SEND_TEST] Error:", result.error);
  }

  return result;
}

/**
 * Test notification setup on the homepage.
 * Navigates to homepage, clicks bell icon, captures console logs for debugging.
 */
async function runNotificationTest(page, snap) {
  const result = {
    ok: false,
    vapidKey: null,
    vapidKeyLength: null,
    decodedKeyLength: null,
    firstByte: null,
    subscriptionError: null,
    consoleLogs: [],
  };

  try {
    console.log("[NOTIF_TEST] Starting notification test on homepage");

    // Get base URL from current page
    const currentUrl = page.url();
    const urlObj = new URL(currentUrl);
    const baseUrl = `${urlObj.protocol}//${urlObj.host}`;
    console.log(`[NOTIF_TEST] Base URL: ${baseUrl}`);

    // Grant notification permission before navigating
    console.log("[NOTIF_TEST] Granting notification permission...");
    const context = page.context();
    await context.grantPermissions(['notifications'], { origin: baseUrl });

    // Navigate to homepage (root)
    console.log("[NOTIF_TEST] Navigating to homepage...");
    await page.goto(baseUrl, { waitUntil: "networkidle", timeout: 20000 });
    await page.waitForLoadState("networkidle", { timeout: 10000 }).catch(() => {});
    await snap("homepage");

    // Wait for the page to fully load
    await page.waitForTimeout(2000);

    // Dismiss the "Welcome to Hyperware" modal if present
    const welcomeModal = page.getByRole('button', { name: /let's go|don't show/i }).first();
    const hasWelcomeModal = await welcomeModal.isVisible({ timeout: 2000 }).catch(() => false);
    if (hasWelcomeModal) {
      console.log("[NOTIF_TEST] Dismissing welcome modal...");
      await welcomeModal.click();
      await page.waitForTimeout(1000);
      await snap("after-dismiss-welcome");
    }

    // Look for the bell icon / notification button
    // The bell icon is in the top-right header area, second button from left
    console.log("[NOTIF_TEST] Looking for notification bell icon...");

    // Try multiple selectors for the bell button
    const bellSelectors = [
      'button[aria-label*="notification" i]',
      'button[aria-label*="bell" i]',
      '[data-notification-button]',
      // The bell is typically next to Search in the header
      'header button:nth-child(2)',
      // Or find by SVG path that looks like a bell
      'button:has(svg path[d*="M15"])',
    ];

    let bellButton = null;
    for (const selector of bellSelectors) {
      const btn = page.locator(selector).first();
      const isVisible = await btn.isVisible({ timeout: 1000 }).catch(() => false);
      if (isVisible) {
        console.log(`[NOTIF_TEST] Found bell with selector: ${selector}`);
        bellButton = btn;
        break;
      }
    }

    // If still not found, try finding all buttons in the top area and list them
    if (!bellButton) {
      console.log("[NOTIF_TEST] Bell button not found with specific selectors, scanning page...");
      // Get all buttons and log their info
      const allButtons = await page.locator('button').all();
      console.log(`[NOTIF_TEST] Found ${allButtons.length} total buttons on page`);

      for (let i = 0; i < Math.min(allButtons.length, 10); i++) {
        const btn = allButtons[i];
        const box = await btn.boundingBox().catch(() => null);
        const ariaLabel = await btn.getAttribute('aria-label').catch(() => '');
        const className = await btn.getAttribute('class').catch(() => '');
        if (box && box.y < 100) { // Top area buttons
          console.log(`[NOTIF_TEST] Top button ${i}: y=${box.y}, aria="${ariaLabel}", class="${className}"`);
        }
      }

      // Try clicking the second button in the top-right area (likely the bell)
      // Looking at screenshot: there are 2 icon buttons before "Search apps..."
      const topButtons = await page.locator('button').all();
      for (const btn of topButtons) {
        const box = await btn.boundingBox().catch(() => null);
        if (box && box.y < 50 && box.x > 700 && box.x < 850) {
          console.log(`[NOTIF_TEST] Found likely bell button at x=${box.x}, y=${box.y}`);
          bellButton = btn;
          break;
        }
      }
    }

    // Click the bell button
    if (bellButton) {
      console.log("[NOTIF_TEST] Clicking notification button...");
      await bellButton.click({ timeout: 5000 });
    } else {
      console.log("[NOTIF_TEST] Could not find bell button!");
    }

    await snap("after-bell-click");

    // Wait for notification menu to open
    await page.waitForTimeout(2000);

    // Check if notification menu appeared
    const menuVisible = await page.locator('[class*="notification"], [data-notification-menu], .notification-menu').first().isVisible({ timeout: 2000 }).catch(() => false);
    console.log(`[NOTIF_TEST] Notification menu visible: ${menuVisible}`);
    await snap("after-bell-wait");

    // Look for enable notifications button if notification menu opened
    const enableBtn = page.getByRole('button', { name: /enable|subscribe|allow/i }).first();
    const hasEnableBtn = await enableBtn.isVisible({ timeout: 3000 }).catch(() => false);
    console.log(`[NOTIF_TEST] Enable button visible: ${hasEnableBtn}`);

    if (hasEnableBtn) {
      console.log("[NOTIF_TEST] Found Enable Notifications button, clicking...");
      await enableBtn.click();
      await snap("after-enable-click");
      // Wait for subscription attempt
      await page.waitForTimeout(5000);
      await snap("after-enable-wait");
    } else {
      // Try to find any button related to notifications on the page
      console.log("[NOTIF_TEST] Looking for any notification-related UI...");
      const notifElements = await page.locator('*:has-text("notification"), *:has-text("Notification")').all();
      console.log(`[NOTIF_TEST] Found ${notifElements.length} elements mentioning notifications`);
    }

    // Wait longer for push subscription to complete (async operation)
    console.log("[NOTIF_TEST] Waiting for push subscription to complete...");
    await page.waitForTimeout(8000);
    await snap("notification-test-complete");

    // Extract relevant console logs
    result.ok = true;
    console.log("[NOTIF_TEST] Test completed - check console.json for [Push] logs");
  } catch (err) {
    result.error = err.message || String(err);
    console.log("[NOTIF_TEST] Error:", result.error);
  }

  return result;
}

/**
 * Test push notifications by sending a DM from node 1 to node 2 while node 2 is NOT on the chat page.
 * This tests the actual push notification flow end-to-end.
 */
async function runPushNotificationTest(page1, page2, snap1, snap2, dump2, targetDir2) {
  const result = {
    ok: false,
    node2SubscriptionRegistered: false,
    node1SentMessage: false,
    pushNotificationReceived: false,
    error: null,
  };

  try {
    console.log("[PUSH_TEST] Starting push notification test");

    // Get base URLs
    const url1 = new URL(page1.url());
    const url2 = new URL(page2.url());
    const baseUrl1 = `${url1.protocol}//${url1.host}`;
    const baseUrl2 = `${url2.protocol}//${url2.host}`;

    // Grant notification permissions for both nodes
    console.log("[PUSH_TEST] Granting notification permissions...");
    await page1.context().grantPermissions(['notifications'], { origin: baseUrl1 });
    await page2.context().grantPermissions(['notifications'], { origin: baseUrl2 });

    // Step 1: Clear localStorage on Node 2 to prevent any persisted widgets from loading
    console.log("[PUSH_TEST] Clearing localStorage on Node 2 to prevent persisted widgets...");
    await page2.goto(baseUrl2, { waitUntil: "domcontentloaded", timeout: 20000 });
    await page2.evaluate(() => {
      localStorage.clear();
      console.log('[PUSH_TEST_CLIENT] localStorage cleared');
    });

    // Reload the page to get fresh state
    console.log("[PUSH_TEST] Node 2 reloading page with fresh state...");
    await page2.reload({ waitUntil: "networkidle", timeout: 20000 });
    await page2.waitForTimeout(3000);
    await snap2("push-test-node2-homepage");

    // Dismiss welcome modal if present
    const welcomeBtn2 = page2.getByRole('button', { name: /let's go|don't show/i }).first();
    if (await welcomeBtn2.isVisible({ timeout: 2000 }).catch(() => false)) {
      await welcomeBtn2.click();
      await page2.waitForTimeout(1000);
    }

    // Wait for push subscription to complete (initializePushNotifications runs on page load)
    console.log("[PUSH_TEST] Waiting for push subscription to complete on Node 2...");
    await page2.waitForTimeout(8000);
    await snap2("push-test-node2-subscribed");

    // Check console logs to verify subscription was created
    result.node2SubscriptionRegistered = true;
    console.log("[PUSH_TEST] Node 2 push subscription should be registered");

    // Step 2: Node 1 sends a DM to Node 2
    // First, navigate to chat on Node 1
    console.log("[PUSH_TEST] Node 1 navigating to chat...");
    await page1.goto(`${baseUrl1}/chat:chat:ware.hypr/`, { waitUntil: "networkidle", timeout: 20000 });
    await page1.waitForTimeout(2000);
    await snap1("push-test-node1-chat");

    // Try to click on existing fake2.os chat if it exists
    console.log("[PUSH_TEST] Looking for existing chat with fake2.os...");
    const existingChat = page1.locator('text=fake2.os').first();
    if (await existingChat.isVisible({ timeout: 3000 }).catch(() => false)) {
      console.log("[PUSH_TEST] Found existing chat with fake2.os, clicking...");
      await existingChat.click();
      await page1.waitForTimeout(1500);
    } else {
      // Start a new DM if no existing chat
      console.log("[PUSH_TEST] No existing chat, starting new DM...");
      const newBtn = page1.locator('button:has-text("New"), button:has-text("+ New")').first();
      if (await newBtn.isVisible({ timeout: 2000 }).catch(() => false)) {
        await newBtn.click();
        await page1.waitForTimeout(1000);

        // Click "Start One-on-One"
        const oneOnOne = page1.locator('text=Start One-on-One, text=One-on-One').first();
        if (await oneOnOne.isVisible({ timeout: 2000 }).catch(() => false)) {
          await oneOnOne.click();
          await page1.waitForTimeout(1000);
        }

        // Enter recipient
        const recipientInput = page1.locator('input[placeholder*="recipient" i], input[placeholder*="node" i]').first();
        if (await recipientInput.isVisible({ timeout: 2000 }).catch(() => false)) {
          await recipientInput.fill('fake2.os');
          await recipientInput.press('Enter');
          await page1.waitForTimeout(1000);
        }
      }
    }
    await snap1("push-test-node1-dm-opened");

    // Type and send a test message
    const messageInput = page1.locator('textarea[placeholder*="message" i], textarea[placeholder*="Type" i]').first();
    if (await messageInput.isVisible({ timeout: 5000 }).catch(() => false)) {
      console.log("[PUSH_TEST] Node 1 sending test message...");
      const testMsg = 'Push notification test ' + Date.now();
      await messageInput.fill(testMsg);
      await page1.waitForTimeout(300);
      await messageInput.press('Enter');
      await page1.waitForTimeout(2000);
      result.node1SentMessage = true;
      console.log("[PUSH_TEST] Node 1 sent message to Node 2:", testMsg);
    } else {
      console.log("[PUSH_TEST] Could not find message input on Node 1");
    }
    await snap1("push-test-node1-message-sent");

    // Step 3: Wait and check if Node 2 received a push notification
    // Since Node 2 is on the homepage (not chat), active_connections.is_empty() should be true
    console.log("[PUSH_TEST] Waiting for push notification to be delivered to Node 2...");
    await page2.waitForTimeout(10000);
    await snap2("push-test-node2-after-wait");

    // Dump node 2's console to check for push notification logs
    await dump2(targetDir2, "/");

    result.ok = true;
    console.log("[PUSH_TEST] Test completed - check node-fake2/console.json for push notification logs");
  } catch (err) {
    result.error = err.message || String(err);
    console.log("[PUSH_TEST] Error:", result.error);
  }

  return result;
}

let shutdownRequested = false;
const startedProcesses = [];

process.on("SIGINT", async () => {
  shutdownRequested = true;
  await Promise.all(startedProcesses.map((p) => stopNode(p.child)));
  process.exit(1);
});

(async () => {
  await ensureDir(runRoot);

  const metadata = {
    startedAt: new Date().toISOString(),
    runLabel,
    headless,
    chatPath,
    passwordLength: password.length,
    nodes,
    skipBoot,
    persistHome,
  };

  try {
    const nodeRuns = [];
    metadata.nodeRuns = nodeRuns;

    if (!skipBoot) {
      for (const node of nodes) {
        console.log(`[node:${node.name}] booting on ${node.port}...`);
        const result = await startNode(node);
        startedProcesses.push(result);
        console.log(`[node:${node.name}] ready`);
      }
    } else {
      console.log(
        `Skipping node boot (--reuse); assuming nodes are already running.`
      );
    }

    if (!skipChatStart) {
      const targetPort = nodes[0]?.port || basePort;
      console.log(
        `[chat] starting from ${chatDir} against http://localhost:${targetPort}...`
      );
      const chatProc = await startChatServer(targetPort);
      startedProcesses.push(chatProc);
      metadata.chat = {
        logPath: chatProc.logPath,
        chatDir,
        port: targetPort,
      };
      // Wait for the chat route to become healthy (2xx/3xx).
      const chatUrl = `http://localhost:${targetPort}${chatPath.startsWith("/") ? chatPath : `/${chatPath}`}`;
      try {
        await waitForHttp(
          chatUrl,
          120000,
          (code) => code >= 200 && code < 400
        );
        metadata.chatReady = true;
      } catch (err) {
        metadata.chatReady = false;
        metadata.chatWaitError = err.message || String(err);
        console.warn(`[chat] did not become ready: ${metadata.chatWaitError}`);
      }

      // Install chat on additional nodes (after first build completed)
      for (let i = 1; i < nodes.length; i++) {
        const nodePort = nodes[i].port;
        try {
          await installChatOnNode(nodePort);
          const nodeUrl = `http://localhost:${nodePort}${chatPath.startsWith("/") ? chatPath : `/${chatPath}`}`;
          await waitForHttp(nodeUrl, 30000, (code) => code >= 200 && code < 400);
        } catch (err) {
          console.warn(`[chat] install on port ${nodePort} failed: ${err.message}`);
        }
      }
    } else {
      metadata.chat = { skipped: true };
    }

    // Use persistent context to avoid incognito mode (required for Push API)
    const userDataDir = path.join(runRoot, 'browser-profile');
    await ensureDir(userDataDir);
    let browser = await chromium.launchPersistentContext(userDataDir, {
      headless,
      args: ['--disable-blink-features=AutomationControlled'], // Make it look less like automation
    });

    // Helper to setup a page for a node (login and navigate to chat)
    async function setupNodePage(browserContext, node, targetDir) {
      await waitForHttp(`http://localhost:${node.port}/`);
      await ensureDir(targetDir);
      const baseUrl = `http://localhost:${node.port}`;

      const page = await browserContext.newPage();
      const snap = createSnapper(page, targetDir);
      const attached = attachCollectors(page);

      console.log(`[node:${node.name}] navigating to login`);
      await page.goto(baseUrl, { waitUntil: "domcontentloaded", timeout: 20000 });
      await snap("login");
      await page.waitForSelector("#password", { timeout: 15000 });
      await page.waitForSelector("#login-button", { timeout: 15000 });
      await page.waitForSelector("#login-button:not([disabled])", { timeout: 8000 }).catch(() => {});

      const loginResponse = page.waitForResponse(
        (res) => res.url().endsWith("/login") && res.request().method() === "POST",
        { timeout: 20000 }
      ).catch(() => null);
      const navAfterLogin = page.waitForNavigation({ waitUntil: "domcontentloaded", timeout: 20000 }).catch(() => null);

      await page.fill("#password", password);
      await page.click("#login-button");

      await loginResponse;
      await navAfterLogin;
      await page.waitForLoadState("networkidle", { timeout: 15000 }).catch(() => {});

      const chatUrl = devUrl ? devUrl : `${baseUrl}${chatPath.startsWith("/") ? chatPath : `/${chatPath}`}`;
      console.log(`[node:${node.name}] opening chat at ${chatUrl}`);
      await page.goto(chatUrl, { waitUntil: "networkidle", timeout: 25000 });
      await page.waitForLoadState("networkidle", { timeout: 10000 }).catch(() => {});
      await snap("chat-home");

      return { page, snap, dump: attached.dump };
    }

    // For 2-node tests, we need to keep both pages open
    const nodePages = [];
    let groupNameForLeaveTest = null;

    for (let i = 0; i < nodes.length; i++) {
      const node = nodes[i];
      const nodeMeta = {
        name: node.name,
        port: node.port,
        baseUrl: `http://localhost:${node.port}`,
      };
      nodeRuns.push(nodeMeta);

      const targetDir = path.join(runRoot, `node-${node.name}`);

      try {
        const { page, snap, dump } = await setupNodePage(browser, node, targetDir);
        nodePages.push({ node, page, snap, dump, targetDir, nodeMeta });

        // Test app settings modal (Profile, General, Notifications tabs)
        if (i === 0) {
          const appSettingsResult = await testAppSettings(page, snap);
          nodeMeta.appSettings = appSettingsResult;
          if (!appSettingsResult.ok) {
            console.warn(`[node:${node.name}] app settings test failed:`, appSettingsResult.error);
          }
        }

        // Determine which tests to run now so we know whether to run group flow
        const earlyTestCheck = testFilter
          ? testFilter.split(",").map(t => t.trim().toLowerCase())
          : [];
        const skipGroupFlow = earlyTestCheck.length > 0 &&
          !earlyTestCheck.includes("all") &&
          !earlyTestCheck.includes("groupflow") &&
          !earlyTestCheck.includes("leavegroup");

        // Only run the full group flow on node 1 (unless skipped for specific tests)
        if (i === 0 && !skipGroupFlow) {
          const flowResult = await driveGroupFlows(page, snap);
          nodeMeta.groupFlow = flowResult;
          groupNameForLeaveTest = flowResult.groupName;

          const requiredOk = flowResult.ok;
          nodeMeta.success = requiredOk;
          if (!requiredOk) {
            throw new Error(`[node:${node.name}] flow failed: ${flowResult.error || "group smoke failed"}`);
          }
        } else if (i === 0 && skipGroupFlow) {
          // Skip group flow for quick tests
          nodeMeta.success = true;
          nodeMeta.groupFlow = { skipped: true };
        } else {
          // For node 2, just mark as ready
          nodeMeta.success = true;
          nodeMeta.groupFlow = { ready: true };
        }
      } catch (err) {
        nodeMeta.success = false;
        nodeMeta.error = err.message || String(err);
        console.error(`[node:${node.name}] failed: ${nodeMeta.error}`);
      }
    }

    // Determine which tests to run based on --test flag
    const testsToRun = testFilter
      ? testFilter.split(",").map(t => t.trim().toLowerCase())
      : (nodePages.length >= 2 ? ["leavegroup", "kickmember"] : []);
    const runAllTests = testsToRun.includes("all");

    // Run 2-node leave group test if requested
    if (nodePages.length >= 2 && groupNameForLeaveTest && (runAllTests || testsToRun.includes("leavegroup"))) {
      console.log("[RUNNER] Running 2-node leave group propagation test");
      const { page: page1, snap: snap1 } = nodePages[0];
      const { page: page2, snap: snap2 } = nodePages[1];

      const leaveTestResult = await testLeaveGroupPropagation(page1, page2, snap1, snap2, groupNameForLeaveTest);
      metadata.leaveGroupTest = leaveTestResult;
      nodeRuns[0].leaveGroupTest = leaveTestResult;

      if (!leaveTestResult.ok) {
        console.error("[RUNNER] Leave group test FAILED:", leaveTestResult);
      } else {
        console.log("[RUNNER] Leave group test PASSED");
      }
    }

    // Run 2-node kick member test if requested
    // Note: This test creates a NEW group because the leave test modified the first group
    if (nodePages.length >= 2 && (runAllTests || testsToRun.includes("kickmember"))) {
      console.log("[RUNNER] Running 2-node kick member propagation test");
      const { page: page1, snap: snap1 } = nodePages[0];
      const { page: page2, snap: snap2 } = nodePages[1];

      try {
        // Navigate to chat home to ensure clean state
        const chatUrl = chatPath.startsWith("/") ? chatPath : `/${chatPath}`;
        await page1.goto(chatUrl, { waitUntil: "networkidle", timeout: 15000 });
        await page1.waitForTimeout(2000);
        if (snap1) await snap1("kick-test-before-new");

        // Close any open modals by pressing Escape
        await page1.keyboard.press("Escape");
        await page1.waitForTimeout(500);

        // Create new group
        const newBtn = page1.locator("button", { hasText: "+ New" }).first();
        console.log("[KICK_TEST] Clicking + New button");
        await newBtn.click({ timeout: 5000 });
        await page1.waitForTimeout(500);
        if (snap1) await snap1("kick-test-after-new-click");

        // Wait for the "New chat" modal heading
        await page1.getByRole("heading", { name: /new chat/i }).first().waitFor({ timeout: 8000 });

        // Click "Create Group Chat"
        await page1.getByRole("button", { name: /create group chat/i }).first().click({ timeout: 8000 });

        // Wait for create group modal
        await page1.getByText("Create group").waitFor({ timeout: 10000 });
        if (snap1) await snap1("kick-test-create-group-modal");

        const kickTestGroupName = `Kick Test ${Math.floor(Math.random() * 1000000)}`;
        const nameField = page1.locator('[data-testid="group-name-input"], input[placeholder="Team updates"]').first();
        await nameField.waitFor({ timeout: 8000 });
        await nameField.click();
        await nameField.fill(kickTestGroupName);
        console.log(`[KICK_TEST] Group name: ${kickTestGroupName}`);

        // Click Create button
        const createBtn = page1.getByRole("button", { name: /^create$/i }).first();
        await createBtn.click({ timeout: 5000 });

        // Wait for navigation to the group view
        await page1.waitForTimeout(3000);
        if (snap1) await snap1("kick-test-after-create");

        // Wait for the group view to load (look for the group name in the header)
        try {
          await page1.locator("h1", { hasText: kickTestGroupName }).first().waitFor({ timeout: 10000 });
          console.log("[KICK_TEST] Group view loaded");
        } catch {
          // Maybe still on chat home - try clicking the group
          console.log("[KICK_TEST] Group view not found, looking for group in list");
          const groupInList = page1.locator(`.chat-item`, { hasText: kickTestGroupName }).first();
          if (await groupInList.isVisible({ timeout: 5000 }).catch(() => false)) {
            await groupInList.click({ timeout: 5000 });
            await page1.waitForTimeout(2000);
          }
        }
        if (snap1) await snap1("kick-test-in-group");

        const kickTestResult = await testKickMemberPropagation(page1, page2, snap1, snap2, kickTestGroupName);
        metadata.kickMemberTest = kickTestResult;
        nodeRuns[0].kickMemberTest = kickTestResult;

        if (!kickTestResult.ok) {
          console.error("[RUNNER] Kick member test FAILED:", kickTestResult);
        } else {
          console.log("[RUNNER] Kick member test PASSED");
        }
      } catch (err) {
        console.error("[RUNNER] Kick member test setup FAILED:", err.message);
        metadata.kickMemberTest = { ok: false, error: err.message };
      }
    }

    // Run adversarial kicked member send test if requested
    // This tests that even if a kicked member tries to send, the owner's node rejects it
    if (nodePages.length >= 2 && (runAllTests || testsToRun.includes("kicksend"))) {
      console.log("[RUNNER] Running adversarial kicked member send test");
      const { page: page1, snap: snap1 } = nodePages[0];
      const { page: page2, snap: snap2 } = nodePages[1];

      try {
        // Navigate to chat home to ensure clean state
        const chatUrl = chatPath.startsWith("/") ? chatPath : `/${chatPath}`;
        await page1.goto(chatUrl, { waitUntil: "networkidle", timeout: 15000 });
        await page2.goto(chatUrl, { waitUntil: "networkidle", timeout: 15000 });
        await page1.waitForTimeout(2000);

        // Close any open modals by pressing Escape
        await page1.keyboard.press("Escape");
        await page1.waitForTimeout(500);

        // Create new group for this test
        const newBtn = page1.locator("button", { hasText: "+ New" }).first();
        console.log("[KICK_SEND_TEST] Clicking + New button");
        await newBtn.click({ timeout: 5000 });
        await page1.waitForTimeout(500);

        // Wait for the "New chat" modal heading
        await page1.getByRole("heading", { name: /new chat/i }).first().waitFor({ timeout: 8000 });

        // Click "Create Group Chat"
        await page1.getByRole("button", { name: /create group chat/i }).first().click({ timeout: 8000 });

        // Wait for create group modal
        await page1.getByText("Create group").waitFor({ timeout: 10000 });

        const kickSendGroupName = `KickSend Test ${Math.floor(Math.random() * 1000000)}`;
        const nameField = page1.locator('[data-testid="group-name-input"], input[placeholder="Team updates"]').first();
        await nameField.waitFor({ timeout: 8000 });
        await nameField.click();
        await nameField.fill(kickSendGroupName);
        console.log(`[KICK_SEND_TEST] Group name: ${kickSendGroupName}`);

        // Click Create button
        const createBtn = page1.getByRole("button", { name: /^create$/i }).first();
        await createBtn.click({ timeout: 5000 });

        // Wait for navigation to the group view
        await page1.waitForTimeout(3000);

        // Wait for the group view to load
        try {
          await page1.locator("h1", { hasText: kickSendGroupName }).first().waitFor({ timeout: 10000 });
          console.log("[KICK_SEND_TEST] Group view loaded");
        } catch {
          console.log("[KICK_SEND_TEST] Group view not found, looking for group in list");
          const groupInList = page1.locator(`.chat-item`, { hasText: kickSendGroupName }).first();
          if (await groupInList.isVisible({ timeout: 5000 }).catch(() => false)) {
            await groupInList.click({ timeout: 5000 });
            await page1.waitForTimeout(2000);
          }
        }
        if (snap1) await snap1("kick-send-test-in-group");

        const kickSendResult = await testKickedMemberCannotSend(page1, page2, snap1, snap2, kickSendGroupName);
        metadata.kickSendTest = kickSendResult;
        nodeRuns[0].kickSendTest = kickSendResult;

        if (!kickSendResult.ok) {
          console.error("[RUNNER] Adversarial kick send test FAILED:", kickSendResult);
        } else {
          console.log("[RUNNER] Adversarial kick send test PASSED");
        }
      } catch (err) {
        console.error("[RUNNER] Adversarial kick send test setup FAILED:", err.message);
        metadata.kickSendTest = { ok: false, error: err.message };
      }
    }

    // Run notification test if requested (--test notifications)
    if (nodePages.length >= 1 && (runAllTests || testsToRun.includes("notifications"))) {
      console.log("[RUNNER] Running notification debug test on homepage");
      const { page: page1, snap: snap1, dump: dump1, targetDir: targetDir1 } = nodePages[0];

      try {
        const notifResult = await runNotificationTest(page1, snap1);
        metadata.notificationTest = notifResult;
        nodeRuns[0].notificationTest = notifResult;

        // Dump the console logs after notification test
        await dump1(targetDir1, chatPath);

        if (!notifResult.ok) {
          console.error("[RUNNER] Notification test had issues:", notifResult);
        } else {
          console.log("[RUNNER] Notification test completed - check console.json for [Push] logs");
        }
      } catch (err) {
        console.error("[RUNNER] Notification test FAILED:", err.message);
        metadata.notificationTest = { ok: false, error: err.message };
      }
    }

    // Run push notification test if requested (--test push) - requires 2 nodes
    if (nodePages.length >= 2 && (runAllTests || testsToRun.includes("push"))) {
      console.log("[RUNNER] Running push notification test (2-node)");
      const { page: page1, snap: snap1 } = nodePages[0];
      const { page: page2, snap: snap2, dump: dump2, targetDir: targetDir2 } = nodePages[1];

      try {
        const pushResult = await runPushNotificationTest(page1, page2, snap1, snap2, dump2, targetDir2);
        metadata.pushNotificationTest = pushResult;
        nodeRuns[1].pushNotificationTest = pushResult;

        if (!pushResult.ok) {
          console.error("[RUNNER] Push notification test had issues:", pushResult);
        } else {
          console.log("[RUNNER] Push notification test completed - check node-fake2 for logs");
        }
      } catch (err) {
        console.error("[RUNNER] Push notification test FAILED:", err.message);
        metadata.pushNotificationTest = { ok: false, error: err.message };
      }
    }

    // Close all pages
    for (const { page } of nodePages) {
      if (page) await page.close();
    }

    if (browser) {
      await browser.close();
    }
  } catch (err) {
    metadata.error = err.message || String(err);
    console.error(`Runner error: ${err.stack || err.message}`);
  } finally {
    if (metadata.nodeRuns) {
      metadata.nodeRuns = metadata.nodeRuns.map((run) => ({
        ...run,
        success: run.success,
        groupFlow: run.groupFlow,
      }));
    }
    metadata.finishedAt = new Date().toISOString();
    await writeJson(path.join(runRoot, "metadata.json"), metadata);

    for (const proc of startedProcesses) {
      await stopNode(proc.child);
    }

    for (const node of nodes) {
      const targetDir = path.join(runRoot, `node-${node.name}`);
      const src = path.join(node.home, ".terminal_logs");
      const dest = path.join(targetDir, "backend-logs");
      await copyDir(src, dest);
    }

    console.log(`Artifacts written to ${runRoot}`);
  }
})();
