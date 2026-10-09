import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import vm from "node:vm";

const source = readFileSync(new URL("./history.js", import.meta.url), "utf8");
const user = (id, text) => ({ id, author: { role: "user" }, content: { content_type: "text", parts: [text] } });
const assistant = (id, text) => ({ ...user(id, text), author: { role: "assistant" } });
const conversation = {
  current_node: "answer",
  mapping: {
    root: { id: "root", parent: null, children: ["prompt"] },
    prompt: { id: "prompt", parent: "root", children: ["answer", "alternate"], message: user("prompt", "First prompt") },
    answer: { id: "answer", parent: "prompt", children: [], message: assistant("answer", "Saved answer") },
    alternate: { id: "alternate", parent: "prompt", children: [], message: assistant("alternate", "Selected answer") }
  }
};

function setup(responses, { pathname = "/c/example", cookie = "", fetchImpl, timers = { setTimeout, clearTimeout } } = {}) {
  const requests = [];
  const queue = [...responses];
  const context = vm.createContext({
    window: {}, location: { pathname }, document: { cookie }, AbortSignal, AbortController, DOMException, ...timers,
    fetch: async (url, options) => {
      requests.push({ url, options });
      options.signal.throwIfAborted();
      if (fetchImpl) return fetchImpl(url, options);
      const next = queue.shift();
      assert.ok(next, `Unexpected request: ${url}`);
      const { status = 200, body = next } = next;
      return { ok: status >= 200 && status < 300, status, json: async () => body };
    }
  });
  vm.runInContext(source, context);
  return {
    requests,
    load: (options = {}) => context.window.__chatgptPdfHistory.load({ signal: new AbortController().signal, ...options })
  };
}
const session = { accessToken: "test-session" };
const page = (messages, older, cursor) => ({ messages, page_info: { has_previous_page: older, start_cursor: cursor } });
const plain = (value) => JSON.parse(JSON.stringify(value));

test("loads every page in chronological order and removes overlapping messages", async () => {
  const app = setup([
    session,
    page([user("u2", "Recent prompt"), assistant("a2", "Final answer")], true, "older / cursor"),
    page([assistant("a1", "Middle answer"), user("u2", "Recent prompt")], true, "oldest"),
    page([user("u1", "Oldest prompt")], false)
  ]);
  const progress = [];
  const result = await app.load({ onProgress: (text) => progress.push(text) });
  assert.deepEqual(plain(result).map((message) => message.id), ["u1", "a1", "u2", "a2"]);
  assert.match(app.requests[2].url, /before=older%20%2F%20cursor$/);
  assert.equal(app.requests[1].options.headers.Authorization, "Bearer test-session");
  assert.equal(app.requests[1].options.credentials, "include");
  assert.equal(progress.length, 3);
});

test("legacy endpoint follows the active branch and excludes alternate answers", async () => {
  const app = setup([session, { status: 404 }, conversation]);
  const result = await app.load();
  assert.deepEqual(plain(result).map((message) => message.text), ["First prompt", "Saved answer"]);
  assert.equal(app.requests[2].url, "/backend-api/conversation/example");
});

test("uses the branch selected in the page", async () => {
  const app = setup([session, conversation]);
  const result = await app.load({ selectedIds: ["prompt", "alternate"] });
  assert.deepEqual(plain(result).map((message) => message.text), ["First prompt", "Selected answer"]);
});

test("does not guess a leaf when the selected alternate branch is ambiguous", async () => {
  const data = structuredClone(conversation);
  data.mapping.alternate.children = ["one", "two"];
  const app = setup([session, data]);
  await assert.rejects(app.load({ selectedIds: ["alternate"] }), /selected branch/);
});

test("rejects truncated mapping history instead of presenting a partial chat", async () => {
  const data = structuredClone(conversation);
  delete data.mapping.prompt;
  const app = setup([session, data]);
  await assert.rejects(app.load(), /incomplete conversation history/);
});

test("rejects cyclic mapping history", async () => {
  const data = structuredClone(conversation);
  data.mapping.prompt.parent = "answer";
  const app = setup([session, data]);
  await assert.rejects(app.load(), /incomplete conversation history/);
});

test("rejects missing and repeated pagination cursors", async () => {
  const missing = setup([session, page([user("u", "Prompt")], true)]);
  await assert.rejects(missing.load(), /before the beginning/);
  const repeated = setup([session, page([user("u2", "Latest")], true, "same"), page([user("u1", "Older")], true, "same")]);
  await assert.rejects(repeated.load(), /before the beginning/);
});

test("rejects a later page failure and missing pagination proof", async () => {
  const failed = setup([session, page([user("u", "Prompt")], true, "cursor"), { status: 500 }]);
  await assert.rejects(failed.load(), /HTTP 500/);
  const unproven = setup([session, { messages: [user("u", "Prompt")] }]);
  await assert.rejects(unproven.load(), /did not confirm/);
});

test("paginated parent links preserve branch selection", async () => {
  const nodes = Object.values(conversation.mapping);
  const app = setup([session, { ...page(nodes.slice(2), true, "older"), current_node: "answer" }, page(nodes.slice(0, 2), false)]);
  const result = await app.load();
  assert.deepEqual(plain(result).map((message) => message.id), ["prompt", "answer"]);
});

test("filters internal messages and labels media without dropping its turn", async () => {
  const hidden = { ...assistant("hidden", "Hidden"), metadata: { is_visually_hidden_from_conversation: true } };
  const thoughts = { ...assistant("thoughts", "Internal"), content: { content_type: "thoughts", parts: ["Internal"] } };
  const tool = { ...assistant("tool", "Tool command"), recipient: "browser" };
  const image = { ...user("image", ""), content: { content_type: "multimodal_text", parts: ["Describe", { content_type: "image_asset_pointer" }] } };
  const app = setup([session, page([hidden, thoughts, tool, image, assistant("final", "Answer")], false)]);
  const result = await app.load();
  assert.deepEqual(plain(result).map((message) => message.id), ["image", "final"]);
  assert.match(result[0].text, /Describe\n\n\[Image:/);
  assert.equal(result[0].hasMedia, true);
});

test("temporary and unsaved chats fail without scraping the welcome screen", async () => {
  const app = setup([], { pathname: "/" });
  await assert.rejects(app.load(), /saved conversation/);
  assert.equal(app.requests.length, 0);
});

test("shared chats and project chat URLs resolve the requested conversation", async () => {
  const shared = setup([conversation], { pathname: "/share/example" });
  assert.equal((await shared.load()).length, 2);
  assert.equal(shared.requests[0].url, "/backend-api/share/example");
  const project = setup([session, conversation], { pathname: "/g/g-p-project/c/example" });
  assert.equal((await project.load()).length, 2);
});

test("uses the selected workspace account for history requests", async () => {
  const app = setup([session, { accounts: { workspace: { account: { account_id: "account-id" } } } }, conversation], { cookie: "other=x; _account=workspace" });
  await app.load();
  assert.equal(app.requests[2].options.headers["ChatGPT-Account-ID"], "account-id");
});

test("closing the reader aborts an in-flight history request", async () => {
  const controller = new AbortController();
  const app = setup([], { fetchImpl: (url, { signal }) => new Promise((resolve, reject) => {
    signal.addEventListener("abort", () => reject(signal.reason), { once: true });
  }) });
  const loading = app.load({ signal: controller.signal });
  controller.abort();
  await assert.rejects(loading, { name: "AbortError" });
});

function trackTimers() {
  const pending = new Set();
  return {
    pending,
    timers: {
      setTimeout(callback, delay) {
        const id = setTimeout(() => { pending.delete(id); callback(); }, delay);
        pending.add(id);
        return id;
      },
      clearTimeout(id) { pending.delete(id); clearTimeout(id); }
    }
  };
}

test("history requests leave no timeout timers after success or failure", async () => {
  for (const responses of [[session, conversation], [session, { status: 403 }]]) {
    const tracked = trackTimers();
    const app = setup(responses, { timers: tracked.timers });
    try { await app.load(); } catch (error) { assert.match(error.message, /HTTP 403/); }
    assert.equal(tracked.pending.size, 0);
  }
});

test("cancelling while a response body loads clears its timeout immediately", async () => {
  const tracked = trackTimers();
  const controller = new AbortController();
  const app = setup([], { timers: tracked.timers, fetchImpl: async (url, { signal }) => ({
    ok: true, status: 200,
    json: () => new Promise((resolve, reject) => {
      signal.throwIfAborted();
      signal.addEventListener("abort", () => reject(signal.reason), { once: true });
    })
  }) });
  const loading = app.load({ signal: controller.signal });
  assert.equal(tracked.pending.size, 1);
  await Promise.resolve();
  await Promise.resolve();
  controller.abort();
  await assert.rejects(loading, { name: "AbortError" });
  assert.equal(tracked.pending.size, 0);
});

test("a request timeout aborts fetching and releases its timer", async () => {
  const callbacks = new Map();
  const app = setup([], {
    timers: { setTimeout(callback) { callbacks.set(1, callback); return 1; }, clearTimeout(id) { callbacks.delete(id); } },
    fetchImpl: (url, { signal }) => new Promise((resolve, reject) => {
      signal.addEventListener("abort", () => reject(signal.reason), { once: true });
    })
  });
  const loading = app.load();
  callbacks.get(1)();
  await assert.rejects(loading, { name: "TimeoutError" });
  assert.equal(callbacks.size, 0);
});
