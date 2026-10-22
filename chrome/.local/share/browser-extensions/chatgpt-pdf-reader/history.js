(() => {
  const HIDDEN_TYPES = new Set([
    "thoughts", "reasoning", "reasoning_recap", "model_editable_context", "user_editable_context"
  ]);

  function activeMessages(conversation, selectedIds = []) {
    const mapping = conversation.mapping;
    if (!mapping || !conversation.current_node) throw new Error("ChatGPT returned an unsupported conversation format.");
    function pathFrom(id) {
      const path = [];
      const seen = new Set();
      while (id) {
        if (seen.has(id) || !Object.hasOwn(mapping, id)) {
          throw new Error("ChatGPT returned incomplete conversation history.");
        }
        seen.add(id);
        const node = mapping[id];
        path.push(node);
        if (node.parent === undefined) throw new Error("ChatGPT returned incomplete conversation history.");
        id = node.parent;
      }
      return path.reverse();
    }
    let path = pathFrom(conversation.current_node);
    // A regenerated answer selected in the page can differ from the saved leaf.
    const selected = selectedIds.findLast((id) => Object.hasOwn(mapping, id));
    if (selected && !path.some((node) => node.id === selected || node.message?.id === selected)) {
      let leaf = selected;
      const seen = new Set();
      while (mapping[leaf]?.children?.length === 1) {
        if (seen.has(leaf)) throw new Error("ChatGPT returned an invalid conversation branch.");
        seen.add(leaf);
        leaf = mapping[leaf].children[0];
      }
      if (mapping[leaf]?.children?.length > 1) {
        throw new Error("Scroll to the latest reply in the selected branch and reopen the reader.");
      }
      path = pathFrom(leaf);
    }
    return path.map((node) => node.message).filter(Boolean);
  }

  function readableMessage(message) {
    const role = message.author?.role;
    if (!['user', 'assistant'].includes(role) ||
      message.metadata?.is_visually_hidden_from_conversation || message.metadata?.is_visually_hidden ||
      HIDDEN_TYPES.has(message.content?.content_type) ||
      (message.recipient && message.recipient !== "all" && message.recipient !== "assistant")) return null;
    const content = message.content || {};
    const parts = Array.isArray(content.parts) ? content.parts : [content.text ?? content.parts ?? ""];
    let hasMedia = false;
    const text = parts.map((part) => {
      if (typeof part === "string") return part;
      if (!part) return "";
      if (typeof part.text === "string") return part.text;
      if (typeof part.content === "string") return part.content;
      hasMedia = true;
      return `[${part.content_type === "image_asset_pointer" ? "Image" : "Media"}: ${part.name || part.content_type || "unavailable in reader"}]`;
    }).join("\n\n");
    const attachments = (message.metadata?.attachments || []).map((item) =>
      `[Attachment: ${item.name || item.file_name || "file"}]`).join("\n");
    const body = [text, attachments].filter(Boolean).join("\n\n");
    if (!body.trim()) {
      if (!content.content_type || content.content_type === "text") return null;
      return { id: message.id, role, text: `[${content.content_type}: unavailable in reader]`, hasMedia: true };
    }
    return { id: message.id, role, text: body, hasMedia };
  }

  async function load({ signal, selectedIds = [], onProgress = () => {} }) {
    const match = location.pathname.match(/\/(c|share)\/([a-z0-9-]+)(?:\/|$)/i);
    if (!match) throw new Error("Open a saved conversation to load its full history. Temporary and unsaved chats are not supported.");
    const [, kind, id] = match;
    async function request(path, headers = {}) {
      signal.throwIfAborted();
      const timeoutController = new AbortController();
      const timeout = setTimeout(() => timeoutController.abort(new DOMException("History request timed out.", "TimeoutError")), 30000);
      try {
        const response = await fetch(path, {
          credentials: "include", cache: "no-store", headers: { Accept: "application/json", ...headers },
          signal: AbortSignal.any([signal, timeoutController.signal])
        });
        const data = response.ok ? await response.json() : undefined;
        if (!response.ok) await response.body?.cancel();
        return { ok: response.ok, status: response.status, data };
      } finally {
        clearTimeout(timeout);
      }
    }
    async function json(response) {
      if (!response.ok) throw new Error(`ChatGPT could not load the full history (HTTP ${response.status}).`);
      return response.data;
    }
    let headers = {};
    if (kind === "c") {
      onProgress("Loading full conversation…");
      const session = await json(await request("/api/auth/session"));
      const token = session.accessToken || session.access_token;
      if (!token) throw new Error("Sign in to ChatGPT again, then reopen the reader.");
      headers = { Authorization: `Bearer ${token}` };
      const workspace = document.cookie.match(/(?:^|;\s*)_account=([^;]+)/)?.[1];
      if (workspace) {
        const accounts = await json(await request("/backend-api/accounts/check/v4-2023-04-27", headers));
        const account = accounts.accounts?.[decodeURIComponent(workspace)]?.account?.account_id;
        if (account) headers["ChatGPT-Account-ID"] = account;
      }
    }
    const base = kind === "share" ? `/backend-api/share/${encodeURIComponent(id)}` :
      `/backend-api/conversations/${encodeURIComponent(id)}?num_turns=100&include_has_versions=true`;
    let response = await request(base, headers);
    if (kind === "c" && [404, 405].includes(response.status)) {
      response = await request(`/backend-api/conversation/${encodeURIComponent(id)}`, headers);
    }
    let first = await json(response);
    if (kind === "c" && !first.mapping && !Array.isArray(first.messages)) {
      first = await json(await request(`/backend-api/conversation/${encodeURIComponent(id)}`, headers));
    }
    let data = first;
    let messages;
    if (first.mapping) {
      if (first.page_info?.has_previous_page) throw new Error("ChatGPT returned incomplete conversation history.");
      messages = activeMessages(first, selectedIds);
    } else {
      const pages = [];
      const cursors = new Set();
      while (true) {
        if (!Array.isArray(data.messages) || typeof data.page_info?.has_previous_page !== "boolean") {
          throw new Error("ChatGPT did not confirm that the full conversation was loaded.");
        }
        pages.unshift(data.messages);
        if (!data.page_info.has_previous_page) break;
        const cursor = data.page_info.start_cursor;
        if (!cursor || cursors.has(cursor) || cursors.size >= 1000) {
          throw new Error("ChatGPT stopped returning older messages before the beginning of the conversation.");
        }
        cursors.add(cursor);
        onProgress(`Loading older messages… (${pages.reduce((sum, page) => sum + page.length, 0)} loaded)`);
        data = await json(await request(`${base}${base.includes('?') ? '&' : '?'}before=${encodeURIComponent(cursor)}`, headers));
      }
      const seen = new Set();
      const records = pages.flat().filter((item) => {
        const id = item.message?.id || item.id;
        if (!id) throw new Error("ChatGPT returned a message without an identifier.");
        if (seen.has(id)) return false;
        seen.add(id);
        return true;
      });
      if (records.some((item) => Object.hasOwn(item, "parent") || Object.hasOwn(item, "parent_id"))) {
        const mapping = Object.fromEntries(records.map((item) => [item.id || item.message.id, {
          ...item, parent: Object.hasOwn(item, "parent") ? item.parent : item.parent_id, message: item.message || item
        }]));
        messages = activeMessages({ mapping, current_node: first.current_node }, selectedIds);
      } else {
        // The paginated endpoint returns the active path in chronological order.
        messages = records.map((item) => item.message || item);
      }
    }
    const result = messages.map(readableMessage).filter(Boolean);
    if (!result.length) throw new Error("No readable conversation messages were returned by ChatGPT.");
    return result;
  }

  // Build inert DOM directly. Raw HTML stays text and cannot execute in the reader.
  function markdown(text) {
    const root = document.createElement("div");
    const expressions = [];
    let marker = "\uE000PDFMATH";
    while (text.includes(marker)) marker += "_";
    text = (window.__chatgptPdfMath?.split(text) || [{ type: "text", raw: text }]).map((part) => {
      if (part.type !== "math") return part.raw;
      expressions.push(part.raw);
      return `${marker}${expressions.length - 1}\uE001`;
    }).join("");
    const restoreExpressions = (text) => text.replace(new RegExp(`${marker}(\\d+)\uE001`, "g"),
      (_, index) => expressions[Number(index)]);
    function inline(target, text, depth = 0) {
      if (depth > 12) { target.append(text); return; }
      const syntax = /(`+)([\s\S]*?)\1|(!?)\[([^\]\n]*)\]\(([^\s)]+)\)|(\*\*|~~|\*|(?<![\p{L}\p{N}])__?)(\S[\s\S]*?)\6(?!(?<=_)[\p{L}\p{N}])|\\\[[\s\S]*?\\\]|\\\([\s\S]*?\\\)|\\([\\`*_{}\[\]()#+.!>|-])/;
      const tokens = new RegExp(`${syntax.source}|${marker}(\\d+)\uE001`, "gu");
      let offset = 0;
      for (const match of text.matchAll(tokens)) {
        target.append(text.slice(offset, match.index));
        if (match[1]) {
          const code = document.createElement("code");
          code.textContent = match[2];
          target.append(code);
        } else if (match[3] !== undefined) {
          if (/^(https?:\/\/|mailto:)/i.test(match[5])) {
            const element = document.createElement(match[3] ? "img" : "a");
            if (match[3]) { element.src = match[5]; element.alt = match[4]; }
            else { element.href = match[5]; element.rel = "noopener noreferrer"; element.target = "_blank"; inline(element, match[4], depth + 1); }
            target.append(element);
          } else target.append(match[0]);
        } else if (match[6]) {
          const element = document.createElement(match[6] === "~~" ? "del" : match[6].length === 2 ? "strong" : "em");
          inline(element, match[7], depth + 1);
          target.append(element);
        } else if (match[9] !== undefined) {
          const span = document.createElement("span");
          span.textContent = expressions[Number(match[9])];
          span.setAttribute("data-math-source", span.textContent);
          target.append(span);
        } else target.append(match[8] ?? match[0]);
        offset = match.index + match[0].length;
      }
      target.append(text.slice(offset));
    }
    const lines = text.replace(/\r\n?/g, "\n").split("\n");
    const cells = (line) => line.trim().replace(/^\|/, "").replace(/\|$/, "").split(/(?<!\\)\|/).map((cell) => cell.trim());
    const listItem = (line) => /^(\s*)([-+*]|\d+[.)])\s+(.*)$/.exec(line);
    const special = (index) => /^\s*$|^\s*(`{3,}|~{3,})|^#{1,6}\s|^\s*>|^\s*(?:[-*_]\s*){3,}$/.test(lines[index]) ||
      listItem(lines[index]) || (index + 1 < lines.length && /^\s*(?:=+|-+)\s*$/.test(lines[index + 1]));
    for (let index = 0; index < lines.length;) {
      const line = lines[index];
      if (!line.trim()) { index++; continue; }
      const fence = /^\s*(`{3,}|~{3,})(.*)$/.exec(line);
      if (fence) {
        const body = [];
        index++;
        while (index < lines.length && !new RegExp(`^\\s*${fence[1][0]}{${fence[1].length},}\\s*$`).test(lines[index])) body.push(lines[index++]);
        if (index < lines.length) index++;
        const pre = document.createElement("pre");
        const code = document.createElement("code");
        code.textContent = body.join("\n");
        pre.append(code); root.append(pre); continue;
      }
      const heading = /^(#{1,6})\s+(.*?)(?:\s+#+)?$/.exec(line);
      const setext = index + 1 < lines.length && /^\s*(=+|-+)\s*$/.exec(lines[index + 1]);
      if (heading || setext) {
        const element = document.createElement(`h${heading ? heading[1].length : setext[1][0] === "=" ? 1 : 2}`);
        inline(element, heading ? heading[2] : line); root.append(element); index += heading ? 1 : 2; continue;
      }
      if (/^\s*(?:[-*_]\s*){3,}$/.test(line)) { root.append(document.createElement("hr")); index++; continue; }
      if (/^\s*>/.test(line)) {
        const body = [];
        while (index < lines.length && /^\s*>/.test(lines[index])) body.push(lines[index++].replace(/^\s*> ?/, ""));
        const quote = document.createElement("blockquote");
        quote.append(...markdown(restoreExpressions(body.join("\n"))).childNodes); root.append(quote); continue;
      }
      if (line.includes("|") && index + 1 < lines.length && cells(lines[index + 1]).every((cell) => /^:?-{3,}:?$/.test(cell))) {
        const table = document.createElement("table");
        const head = document.createElement("thead");
        const body = document.createElement("tbody");
        function row(parent, line, tag) {
          const tr = document.createElement("tr");
          for (const cell of cells(line)) { const td = document.createElement(tag); inline(td, cell); tr.append(td); }
          parent.append(tr);
        }
        row(head, line, "th"); index += 2;
        while (index < lines.length && lines[index].trim() && lines[index].includes("|")) row(body, lines[index++], "td");
        table.append(head, body); root.append(table); continue;
      }
      const item = listItem(line);
      if (item) {
        const ordered = /^\d/.test(item[2]);
        const list = document.createElement(ordered ? "ol" : "ul");
        if (ordered) list.start = parseInt(item[2], 10);
        while (index < lines.length) {
          const next = listItem(lines[index]);
          if (!next || next[1].length !== item[1].length || /^\d/.test(next[2]) !== ordered) break;
          const body = [next[3]];
          index++;
          while (index < lines.length && lines[index].trim() && /^\s+/.test(lines[index]) &&
            (!listItem(lines[index]) || listItem(lines[index])[1].length > item[1].length)) {
            body.push(lines[index].slice(next[1].length + next[2].length + 1));
            index++;
          }
          const li = document.createElement("li");
          li.append(...markdown(restoreExpressions(body.join("\n"))).childNodes); list.append(li);
        }
        root.append(list); continue;
      }
      const paragraph = [line]; index++;
      while (index < lines.length && !special(index)) paragraph.push(lines[index++]);
      const p = document.createElement("p");
      paragraph.forEach((line, index) => {
        if (index) p.append(paragraph[index - 1].endsWith("  ") ? document.createElement("br") : "\n");
        inline(p, line);
      });
      root.append(p);
    }
    return root;
  }

  window.__chatgptPdfHistory = { load, markdown };
})();
