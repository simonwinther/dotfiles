(() => {
  const environments = "equation|equation\\*|align|align\\*|alignat|alignat\\*|gather|gather\\*|multline|multline\\*|displaymath";
  const openings = new RegExp(`\\\\(?:\\[|\\(|begin\\{(${environments})\\})|\\$\\$?|\x60+|~{3,}`, "g");

  function escaped(text, index) {
    let slashes = 0;
    while (index > 0 && text[--index] === "\\") slashes++;
    return slashes % 2 === 1;
  }

  // Preserve complete math expressions before Markdown interprets their symbols.
  // Code fences and inline code are skipped; currency is not an inline equation.
  function split(text) {
    const result = [];
    const pattern = new RegExp(openings.source, "g");
    let start = 0;
    let match;
    while ((match = pattern.exec(text))) {
      const left = match[0];
      if (escaped(text, match.index)) continue;
      if (left[0] === "`" || left[0] === "~") {
        const end = text.indexOf(left, pattern.lastIndex);
        pattern.lastIndex = end < 0 ? text.length : end + left.length;
        continue;
      }
      if (left === "$" && (/\s|\$/.test(text[pattern.lastIndex] || "") || !text[pattern.lastIndex])) continue;
      const environment = match[1];
      const right = environment ? `\\end{${environment}}` : left === "\\[" ? "\\]" : left === "\\(" ? "\\)" : left;
      let end = pattern.lastIndex;
      let braces = 0;
      let found = false;
      for (; end < text.length; end++) {
        if (escaped(text, end)) continue;
        if (braces <= 0 && text.startsWith(right, end)) {
          if (left === "$" && (/\s/.test(text[end - 1] || "") || /\d/.test(text[end + 1] || ""))) continue;
          found = true;
          break;
        }
        if (text[end] === "{") braces++;
        else if (text[end] === "}") braces--;
      }
      if (!found) continue;
      if (match.index > start) result.push({ type: "text", raw: text.slice(start, match.index) });
      const raw = text.slice(match.index, end + right.length);
      result.push({
        type: "math", raw,
        expression: environment ? raw : text.slice(pattern.lastIndex, end),
        display: Boolean(environment) || left === "$$" || left === "\\["
      });
      start = end + right.length;
      pattern.lastIndex = start;
    }
    if (start < text.length) result.push({ type: "text", raw: text.slice(start) });
    return result;
  }

  let initialization;
  async function initialize(shadow, { signal } = {}) {
    signal?.throwIfAborted();
    initialization ||= Promise.all([
      import(chrome.runtime.getURL("vendor/temml/temml.js")),
      fetch(chrome.runtime.getURL("vendor/temml/math.css"), { signal }).then((response) => {
        if (!response.ok) throw new Error("The reader's math stylesheet could not be loaded. Reload the extension.");
        return response.text();
      })
    ]).then(([module, css]) => ({ engine: module.default, css })).catch((error) => {
      initialization = undefined;
      throw error;
    });
    const { engine, css } = await initialization;
    signal?.throwIfAborted();
    const style = document.createElement("style");
    style.textContent = css;
    shadow.append(style);
    return engine;
  }

  function render(root, engine, makeText = (text) => document.createTextNode(text)) {
    const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
    const nodes = [];
    let node;
    while ((node = walker.nextNode())) {
      if (!node.parentElement?.closest("pre, code, math, .math-error")) nodes.push(node);
    }
    let rendered = 0;
    let failed = 0;
    for (const node of nodes) {
      const parts = split(node.textContent);
      if (!parts.some((part) => part.type === "math")) continue;
      const fragment = document.createDocumentFragment();
      let offset = 0;
      for (const part of parts) {
        if (part.type === "text") fragment.append(makeText(part.raw, node, offset));
        else {
          const holder = document.createElement("span");
          try {
            engine.render(part.expression, holder, {
              displayMode: part.display, throwOnError: true, trust: false,
              maxExpand: 1000, maxSize: [30, 30], wrap: "none"
            });
            const math = holder.querySelector("math");
            if (!math) throw new Error("No equation was produced.");
            math.setAttribute("aria-label", part.expression);
            math.setAttribute("title", part.expression);
            if (part.display) math.setAttribute("display", "block");
            fragment.append(...holder.childNodes);
            rendered++;
          } catch (error) {
            holder.className = "math-error";
            holder.setAttribute("title", `Could not render equation: ${error.message}`);
            holder.append(makeText(part.raw, node, offset));
            fragment.append(holder);
            failed++;
          }
        }
        offset += part.raw.length;
      }
      node.replaceWith(fragment);
    }
    return { rendered, failed };
  }

  window.__chatgptPdfMath = { split, initialize, render };
})();
