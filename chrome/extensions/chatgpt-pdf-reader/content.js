(() => {
  const KEY = "__chatgptPdfReader";
  if (window[KEY]) {
    window[KEY].close();
    return;
  }

  const WIDTH = 794;
  const HEIGHT = 1123;
  const CONTENT_HEIGHT = 971;
  const BOOKMARK_KEY = "__chatgptPdfReaderBookmark";
  const messages = collectMessages();
  const textLinks = new WeakMap();
  const sourceFragments = new Map();
  let openingAnchor = null;
  let rendered = false;
  let initialReaderTop = 0;
  let initialScale = 1;
  const previousFocus = document.activeElement;
  const host = document.createElement("div");
  host.style.setProperty("all", "initial", "important");
  const shadow = host.attachShadow({ mode: "open" });
  const style = document.createElement("style");
  style.textContent = `
    :host { all: initial; color-scheme: light; }
    * { box-sizing: border-box; }
    dialog { position: fixed; inset: 0; width: 100vw; height: 100dvh; max-width: none;
      max-height: none; margin: 0; padding: 0; border: 0; background: #525659;
      color: #f1f1f1; font: 13px Arial, sans-serif; overflow: hidden; }
    dialog::backdrop { background: #525659; }
    .toolbar { height: 56px; display: flex; align-items: center; gap: 16px; padding: 0 20px;
      background: #323639; box-shadow: 0 2px 5px #0005; position: relative; z-index: 2; }
    .filename { flex: 1; min-width: 0; white-space: nowrap; overflow: hidden;
      text-overflow: ellipsis; font-size: 14px; font-weight: 500; }
    .tools { display: flex; align-items: center; gap: 10px; flex-shrink: 0; }
    .divider { height: 24px; width: 1px; background: #ffffff30; margin: 0 4px; }
    button { display: inline-flex; align-items: center; justify-content: center; width: 32px;
      height: 32px; border: 0; border-radius: 50%; background: transparent; color: inherit;
      cursor: pointer; padding: 6px; font: inherit; }
    button:hover { background: #ffffff20; }
    button:focus-visible, input:focus-visible, .viewport:focus-visible { outline: 2px solid #8ab4f8; outline-offset: -2px; }
    button svg { width: 20px; height: 20px; fill: none; stroke: currentColor; stroke-width: 1.8; }
    input { border: 0; border-radius: 2px; background: #202124; color: #f1f1f1;
      text-align: center; height: 24px; width: 38px; font: inherit; padding: 2px; }
    .zoom { width: 50px; }
    .workspace { height: calc(100% - 56px); display: flex; }
    .outline { width: 230px; flex-shrink: 0; background: #323639; border-right: 1px solid #ffffff20;
      overflow: auto; padding: 16px 12px; }
    .outline[hidden] { display: none; }
    .outline button { display: block; width: 100%; height: auto; border-radius: 4px;
      text-align: left; padding: 12px; line-height: 1.5; overflow-wrap: anywhere; }
    .viewport { overflow: auto; flex: 1; min-width: 0; overscroll-behavior: contain; scrollbar-color: #999 #45494c; }
    .pages { display: flex; flex-direction: column; align-items: center; gap: 16px;
      padding: 20px 24px 32px; width: max-content; min-width: 100%; }
    .page-slot { flex: none; position: relative; }
    .page { position: absolute; top: 0; left: 0; width: ${WIDTH}px; height: ${HEIGHT}px;
      padding: 72px 70px 80px; background: white; color: #171717; transform-origin: top left;
      box-shadow: 0 2px 7px #0007; overflow: hidden; }
    .paper-content { height: ${CONTENT_HEIGHT}px; display: flow-root; font: 16px/1.55 Georgia, 'Times New Roman', serif;
      overflow-wrap: anywhere; text-align: left; }
    .paper-content > :first-child { margin-top: 0; }
    .paper-content p { margin: 0 0 14px; }
    .paper-content h1 { font-size: 27px; line-height: 1.25; margin: 0 0 24px; }
    .paper-content h2 { font-size: 21px; line-height: 1.35; margin: 22px 0 12px; }
    .paper-content h3, .paper-content h4, .paper-content h5, .paper-content h6 { font-size: 17px; line-height: 1.4; margin: 18px 0 10px; }
    .paper-content ul, .paper-content ol { padding-left: 26px; margin: 8px 0 16px; }
    .paper-content li { padding-left: 3px; margin: 4px 0; }
    .paper-content li.continued { list-style: none; }
    .paper-content blockquote { border-left: 2px solid #bbb; margin: 16px 0; padding: 0 18px; color: #444; }
    .paper-content pre { white-space: pre-wrap; font: 12px/1.5 monospace; background: #f4f4f4; padding: 12px; margin: 14px 0; }
    .paper-content code { font: .85em monospace; }
    .paper-content pre code { font: inherit; }
    .paper-content .source-whitespace { white-space: pre-wrap; }
    .paper-content table { border-collapse: collapse; width: 100%; margin: 14px 0; font-size: 13px; table-layout: fixed; }
    .paper-content th, .paper-content td { border: 1px solid #bbb; padding: 7px; vertical-align: top; }
    .paper-content th { background: #f3f3f3; }
    .paper-content img { display: block; max-width: 100%; max-height: 850px; object-fit: contain; margin: 16px auto; }
    .paper-content a { color: inherit; text-decoration-color: #aaa; text-underline-offset: 2px; }
    .paper-content hr { border: 0; border-top: 1px solid #ddd; margin: 24px 0; }
    .paper-content .prompt { font: 14px/1.5 Arial, sans-serif; color: #555; border-bottom: 1px solid #ddd; padding: 0 0 14px; margin: 24px 0 20px; white-space: pre-wrap; }
    .paper-content math { font-family: math; }
    .paper-content math[display="block"] { margin: 18px 0; overflow-wrap: normal; }
    .page-number { position: absolute; bottom: 30px; left: 0; right: 0; text-align: center;
      color: #777; font: 11px Georgia, serif; }
    .status { padding: 60px; color: white; text-align: center; font: 16px Arial, sans-serif; }
    @media (max-width: 650px) { .toolbar { gap: 6px; padding: 0 8px; } .tools { gap: 3px; }
      .divider { margin: 0 2px; } .outline { width: 170px; } }
  `;
  shadow.append(style);
  const dialog = document.createElement("dialog");
  dialog.setAttribute("aria-label", "PDF reader");
  dialog.innerHTML = `
    <header class="toolbar">
      <button data-action="outline" title="Document outline" aria-label="Document outline" aria-expanded="false"></button>
      <span class="filename"></span>
      <div class="tools">
        <input class="page-input" aria-label="Page number" inputmode="numeric" value="1">
        <span class="page-total">/ 1</span><span class="divider"></span>
        <button data-action="minus" title="Zoom out" aria-label="Zoom out"></button>
        <input class="zoom" aria-label="Zoom percentage" value="100%">
        <button data-action="plus" title="Zoom in" aria-label="Zoom in"></button>
        <span class="divider"></span>
        <button data-action="fit" title="Fit to width" aria-label="Fit to width"></button>
        <button data-action="close" title="Close reader (Esc)" aria-label="Close reader"></button>
      </div>
    </header>
    <div class="workspace">
      <nav class="outline" aria-label="Document outline" hidden></nav>
      <main class="viewport" tabindex="0" aria-label="Document pages">
        <div class="pages"><div class="status" role="status">Preparing document…</div></div>
      </main>
    </div>`;
  const icons = {
    outline: '<path d="M3 5h18M3 12h18M3 19h18"/>',
    minus: '<path d="M5 12h14"/>',
    plus: '<path d="M5 12h14M12 5v14"/>',
    fit: '<rect x="5" y="3" width="14" height="18" rx="1"/><path d="M2 12h20m-4-3 3 3-3 3M6 9l-3 3 3 3"/>',
    close: '<path d="m6 6 12 12M6 18 18 6"/>'
  };
  for (const [action, svg] of Object.entries(icons)) {
    dialog.querySelector(`[data-action="${action}"]`).innerHTML = `<svg viewBox="0 0 24 24" aria-hidden="true">${svg}</svg>`;
  }
  shadow.append(dialog);
  document.documentElement.append(host);
  const pagesRoot = shadow.querySelector(".pages");
  const viewport = shadow.querySelector(".viewport");
  const pageInput = shadow.querySelector(".page-input");
  const zoomInput = shadow.querySelector(".zoom");
  const outline = shadow.querySelector(".outline");
  const title = document.title.normalize("NFC")
    .replace(/[\p{Cc}\p{Cf}\p{Co}\uFFFC\uFFFD\uFE00-\uFE0F]/gu, "")
    .replace(/\s*[-–—|]\s*ChatGPT(?:\s+Work)?\s*$/i, "")
    .replace(/[\u25A1\u25AF\u25FB\u25FD\u2610]+\s*$/u, "")
    .replace(/\s+/g, " ").trim() || "Document";
  shadow.querySelector(".filename").textContent = `${title.replace(/\.pdf$/i, "")}.pdf`;
  let closed = false;
  let scale = 1;
  let fitWidth = true;
  let pages = [];
  let currentContent;
  let parentClones = new Map();
  const continuedParents = new WeakSet();
  const state = { close };
  window[KEY] = state;
  const resizeObserver = new ResizeObserver(() => { if (fitWidth) updateZoom(); });

  function close() {
    if (closed) return;
    const reading = rendered ? captureAnchor(readerTextNodes(), viewport.getBoundingClientRect()) : null;
    const link = reading && textLinks.get(reading.node);
    const sourceAnchor = link && { node: link.source, offset: link.start + reading.offset, y: reading.y };
    const moved = rendered && (Math.abs(viewport.scrollTop - initialReaderTop) > 0.5 || scale !== initialScale);
    const readerTop = viewport.scrollTop;
    closed = true;
    resizeObserver.disconnect();
    dialog.close();
    host.remove();
    if (window[KEY] === state) delete window[KEY];
    if (previousFocus?.isConnected) previousFocus.focus({ preventScroll: true });
    if (moved && sourceAnchor) restoreSourceAnchor(sourceAnchor);
    if (sourceAnchor?.node.isConnected) {
      window[BOOKMARK_KEY] = {
        url: location.href,
        node: sourceAnchor.node,
        offset: sourceAnchor.offset,
        y: characterRect(sourceAnchor.node, sourceAnchor.offset)?.top,
        readerTop, scale, fitWidth, outlineHidden: outline.hidden,
        width: innerWidth, height: innerHeight,
        length: sourceTextLength(),
        scrolls: scrollContainers(sourceAnchor.node).map((element) => ({ element, top: element.scrollTop, left: element.scrollLeft }))
      };
    }
  }

  dialog.addEventListener("cancel", (event) => { event.preventDefault(); close(); });
  dialog.addEventListener("close", close);
  dialog.addEventListener("click", (event) => {
    const action = event.target.closest("[data-action]")?.dataset.action;
    if (action === "close") close();
    if (action === "plus" || action === "minus") {
      fitWidth = false;
      scale = Math.min(3, Math.max(0.25, scale + (action === "plus" ? 0.1 : -0.1)));
      updateZoom();
    }
    if (action === "fit") { fitWidth = true; updateZoom(); }
    if (action === "outline") {
      outline.hidden = !outline.hidden;
      event.target.closest("button").setAttribute("aria-expanded", String(!outline.hidden));
      if (fitWidth) updateZoom();
    }
  });
  pageInput.addEventListener("change", () => goToPage(Number(pageInput.value) - 1));
  pageInput.addEventListener("keydown", (event) => { if (event.key === "Enter") { pageInput.blur(); viewport.focus(); } });
  zoomInput.addEventListener("change", () => {
    const value = parseFloat(zoomInput.value);
    if (Number.isFinite(value)) { fitWidth = false; scale = Math.min(3, Math.max(0.25, value / 100)); }
    updateZoom();
  });
  zoomInput.addEventListener("keydown", (event) => { if (event.key === "Enter") { zoomInput.blur(); viewport.focus(); } });
  viewport.addEventListener("scroll", () => {
    if (shadow.activeElement !== pageInput) pageInput.value = String(currentPage() + 1);
  }, { passive: true });

  function currentPage() {
    return Math.max(0, Math.min(pages.length - 1, Math.floor((viewport.scrollTop + 16) / (HEIGHT * scale + 16))));
  }

  function goToPage(index) {
    index = Math.min(pages.length - 1, Math.max(0, Number.isFinite(index) ? Math.floor(index) : 0));
    viewport.scrollTop = Math.max(0, index * (HEIGHT * scale + 16));
    pageInput.value = String(index + 1);
  }

  function updateZoom() {
    const anchor = rendered ? captureAnchor(readerTextNodes(), viewport.getBoundingClientRect()) : null;
    const position = viewport.scrollTop / (HEIGHT * scale + 16);
    if (fitWidth) scale = Math.max(0.25, Math.min(1.25, (viewport.clientWidth - 48) / WIDTH));
    for (const { slot, paper } of pages) {
      slot.style.width = `${WIDTH * scale}px`;
      slot.style.height = `${HEIGHT * scale}px`;
      paper.style.transform = `scale(${scale})`;
    }
    zoomInput.value = `${Math.round(scale * 100)}%`;
    if (anchor) alignReaderAnchor(anchor);
    else viewport.scrollTop = position * (HEIGHT * scale + 16);
  }

  function linkedText(text, source, start = 0) {
    const node = document.createTextNode(text);
    textLinks.set(node, { source, start });
    if (!sourceFragments.has(source)) sourceFragments.set(source, []);
    sourceFragments.get(source).push(node);
    return node;
  }

  function readerTextNodes() {
    return [...sourceFragments.values()].flat().filter((node) => node.isConnected);
  }

  function sourceTextLength() {
    return [...sourceFragments.keys()].reduce((sum, node) => sum + node.length, 0);
  }

  function characterRect(node, offset) {
    if (!node.isConnected || !node.length) return null;
    const range = document.createRange();
    offset = Math.max(0, Math.min(node.length - 1, offset));
    range.setStart(node, offset);
    range.setEnd(node, offset + 1);
    const rect = [...range.getClientRects()].find((item) => item.height > 0);
    return rect || null;
  }

  // Find the first visible line, then a character on that line. Character links
  // survive pagination, so a long paragraph can be followed in both directions.
  function captureAnchor(nodes, bounds) {
    let best = null;
    for (const node of nodes) {
      if (!node.isConnected || !node.textContent.trim()) continue;
      if (node.parentElement?.closest('.sr-only, .katex-mathml, [aria-hidden="true"]')) continue;
      const range = document.createRange();
      range.selectNodeContents(node);
      const line = [...range.getClientRects()].find((rect) => rect.width > 0 && rect.height > 0 &&
        rect.bottom > bounds.top + 1 && rect.top < bounds.bottom &&
        rect.right > bounds.left && rect.left < bounds.right);
      if (!line || (best && (line.top > best.y + 1 || (Math.abs(line.top - best.y) <= 1 && line.left >= best.x)))) continue;
      let low = 0;
      let high = node.length - 1;
      while (low < high) {
        const middle = Math.floor((low + high) / 2);
        const rect = characterRect(node, middle);
        if (!rect || rect.bottom <= line.top + 1) low = middle + 1;
        else high = middle;
      }
      while (low < node.length - 1 && /\s/.test(node.textContent[low])) low++;
      const rect = characterRect(node, low);
      if (rect) best = { node, offset: low, y: rect.top, x: rect.left };
    }
    return best;
  }

  function scrollContainers(node) {
    const containers = [];
    for (let element = node.parentElement; element; element = element.parentElement) {
      if (element !== document.scrollingElement && element.scrollHeight > element.clientHeight + 1 &&
        /auto|scroll|overlay/.test(getComputedStyle(element).overflowY)) containers.push(element);
    }
    if (document.scrollingElement) containers.push(document.scrollingElement);
    return containers;
  }

  function sourceBounds() {
    const source = [...sourceFragments.keys()].find((node) => node.isConnected);
    const scroller = source && scrollContainers(source)[0];
    const rect = scroller && scroller !== document.scrollingElement ? scroller.getBoundingClientRect() :
      { top: 0, bottom: innerHeight, left: 0, right: innerWidth };
    let top = Math.max(56, rect.top);
    // Account for a fixed/sticky conversation header above the reading area.
    for (const header of document.querySelectorAll('header, [role="banner"]')) {
      const box = header.getBoundingClientRect();
      if (box.top <= top && box.bottom < 180 && box.width > 200) top = Math.max(top, box.bottom);
    }
    return { top, bottom: Math.min(innerHeight, rect.bottom), left: Math.max(0, rect.left), right: Math.min(innerWidth, rect.right) };
  }

  function alignReaderAnchor(anchor) {
    const rect = characterRect(anchor.node, anchor.offset);
    if (rect) viewport.scrollTo({ top: viewport.scrollTop + rect.top - anchor.y, behavior: "instant" });
  }

  function restoreReaderAnchor(anchor) {
    if (!anchor) return;
    const fragment = sourceFragments.get(anchor.node)?.find((node) => {
      const link = textLinks.get(node);
      return node.isConnected && anchor.offset >= link.start && anchor.offset < link.start + node.length;
    });
    if (fragment) alignReaderAnchor({ node: fragment, offset: anchor.offset - textLinks.get(fragment).start, y: anchor.y });
  }

  function restoreSourceAnchor(anchor) {
    if (!anchor.node.isConnected) return;
    const bounds = sourceBounds();
    const y = Math.max(bounds.top + 1, Math.min(bounds.bottom - 24, anchor.y));
    for (const element of scrollContainers(anchor.node)) {
      const rect = characterRect(anchor.node, anchor.offset);
      if (!rect || Math.abs(rect.top - y) < 0.5) break;
      element.scrollTo({ top: element.scrollTop + rect.top - y, left: element.scrollLeft, behavior: "instant" });
    }
  }

  function reusableBookmark() {
    const bookmark = window[BOOKMARK_KEY];
    return bookmark && bookmark.url === location.href && bookmark.node.isConnected &&
      bookmark.width === innerWidth && bookmark.height === innerHeight && bookmark.length === sourceTextLength() &&
      Math.abs((characterRect(bookmark.node, bookmark.offset)?.top ?? Infinity) - bookmark.y) < 1 &&
      bookmark.scrolls.every(({ element, top, left }) => element.isConnected && Math.abs(element.scrollTop - top) < 1 && Math.abs(element.scrollLeft - left) < 1)
      ? bookmark : null;
  }

  function collectMessages() {
    const roleSelector = '[data-message-author-role="user"], [data-message-author-role="assistant"]';
    const turnSelector = 'section[data-turn], article[data-turn], [data-testid^="conversation-turn"], [data-turn-id]';
    const contentSelector = '.markdown, .prose, [data-message-content], [data-testid="message-content"]';
    const scope = document.querySelector('main, [role="main"], #thread, [data-testid="conversation"]') || document;
    const owners = new Set();
    const seenRoots = new Set();

    function isVisible(element) {
      if (element.closest('[hidden], [aria-hidden="true"], nav, aside, form, [contenteditable="true"]')) return false;
      const style = getComputedStyle(element);
      return style.display !== "none" && style.visibility !== "hidden";
    }

    // Turn sections survive changes to the inner message wrappers. Also keep
    // support for older pages that have author attributes without turn sections.
    for (const element of scope.querySelectorAll(`${turnSelector}, ${roleSelector}, ${contentSelector}, .whitespace-pre-wrap`)) {
      if (!isVisible(element)) continue;
      const owner = element.closest(turnSelector) || element.closest(roleSelector) || element.closest(contentSelector) || element;
      if (owner !== scope && scope.contains(owner)) owners.add(owner);
    }

    // Some clients render semantic articles without the site's message classes.
    if (!owners.size) {
      for (const article of scope.querySelectorAll("article")) {
        if (isVisible(article) && article.querySelector("p, pre, li, table")) owners.add(article);
      }
    }

    // Keep DOM order even when different kinds of message wrapper coexist.
    const ordered = [...owners].sort((a, b) => a.compareDocumentPosition(b) & Node.DOCUMENT_POSITION_FOLLOWING ? -1 : 1);
    const result = [];
    for (const element of ordered) {
      const author = element.matches(roleSelector) ? element : element.querySelector(roleSelector);
      const turn = element.getAttribute("data-turn") || "";
      const label = element.getAttribute("aria-label") || "";
      const speaker = element.querySelector('h5.sr-only, h6.sr-only, [data-testid="message-role"]')?.textContent || "";
      const plainPrompt = element.matches(".whitespace-pre-wrap") ? element : element.querySelector(".whitespace-pre-wrap:not(pre):not(code)");
      const role = author?.getAttribute("data-message-author-role") ||
        (/^(user|human)$/i.test(turn) || /^(you|user)(\s|:|$)/i.test(label || speaker) ||
          (plainPrompt && !plainPrompt.closest(contentSelector) && !element.querySelector(contentSelector)) ? "user" : "assistant");
      let roots = role === "user" && plainPrompt ? [plainPrompt] :
        element.matches(contentSelector) ? [element] : [...element.querySelectorAll(contentSelector)];
      if (!roots.length) {
        roots = role === "user" ? [element.querySelector(".whitespace-pre-wrap") || element] : [author || element];
      }
      // Nested .prose/.markdown containers and mixed old/new wrappers must not
      // export the same response twice.
      roots = roots.filter((root, index, all) => isVisible(root) &&
        !all.some((other, otherIndex) => otherIndex !== index && other.contains(root)) &&
        ![...seenRoots].some((seen) => seen.contains(root)));
      if (!roots.length) continue;
      for (const root of roots) seenRoots.add(root);
      result.push({ element, role, roots });
    }

    // A final semantic fallback covers clients with neither turn wrappers nor
    // markdown classes. Restrict it to the conversation's main region.
    if (!result.length && scope !== document && scope.querySelector("p, pre, li, table")) {
      result.push({ element: scope, role: "assistant", roots: [scope] });
    }
    return result;
  }

  // Rebuild semantic content only: never copy site controls, event handlers, or styles.
  const allowed = new Set("p div span h1 h2 h3 h4 h5 h6 ul ol li blockquote pre code strong b em i u s del a br hr table thead tbody tfoot tr td th sup sub figure figcaption dl dt dd".split(" "));
  const skipped = "button, script, style, noscript, textarea, input, select, svg, canvas, nav, aside, header, footer, form, [contenteditable], [hidden], [aria-hidden='true'], .sr-only";
  function clean(node) {
    if (node.nodeType === Node.TEXT_NODE) return linkedText(node.textContent, node);
    if (!(node instanceof Element) || node.matches(skipped)) return document.createDocumentFragment();
    if (node.matches(".katex, .katex-display, [data-math-source]")) {
      const math = node.querySelector("math");
      if (math) return clean(math);
      if (node.hasAttribute("data-math-source")) return document.createTextNode(node.getAttribute("data-math-source"));
    }
    if (node.namespaceURI === "http://www.w3.org/1998/Math/MathML") {
      if (node.localName === "annotation" || node.localName === "annotation-xml") return document.createDocumentFragment();
      const math = document.createElementNS(node.namespaceURI, node.localName);
      for (const name of ["display", "mathvariant", "stretchy", "fence", "separator", "columnalign", "rowspacing", "columnspacing", "displaystyle", "scriptlevel", "linethickness", "accent", "accentunder", "notation", "width", "height", "depth", "lspace", "rspace", "minsize", "maxsize", "largeop", "movablelimits"]) {
        if (node.hasAttribute(name)) math.setAttribute(name, node.getAttribute(name));
      }
      for (const child of node.childNodes) math.append(clean(child));
      return math;
    }
    if (node.localName === "img") {
      const img = document.createElement("img");
      const src = node.currentSrc || node.src;
      if (!/^(https?:|data:image\/|blob:)/i.test(src)) return document.createTextNode(node.alt || "");
      img.src = src;
      img.alt = node.alt;
      if (node.naturalWidth && node.naturalHeight) {
        img.width = Math.min(654, node.naturalWidth);
        img.height = img.width * node.naturalHeight / node.naturalWidth;
        img.style.height = "auto";
      }
      return img;
    }
    if (node.localName === "pre") {
      const pre = document.createElement("pre");
      const code = document.createElement("code");
      for (const child of (node.querySelector("code") || node).childNodes) code.append(clean(child));
      pre.append(code);
      return pre;
    }
    const copy = allowed.has(node.localName) ? document.createElement(node.localName) : document.createDocumentFragment();
    if (copy instanceof Element) {
      if (node.matches(".whitespace-pre-wrap") || /^(pre-wrap|pre-line|break-spaces)$/.test(getComputedStyle(node).whiteSpace)) {
        copy.classList.add("source-whitespace");
      }
      for (const attr of ["start", "value", "colspan", "rowspan", "dir", "lang"]) {
        if (node.hasAttribute(attr)) copy.setAttribute(attr, node.getAttribute(attr));
      }
      if (node.localName === "a" && /^(https?:|mailto:)/i.test(node.href)) {
        copy.setAttribute("href", node.href);
        copy.setAttribute("target", "_blank");
        copy.setAttribute("rel", "noopener noreferrer");
      }
      if (node.localName === "li" && node.parentElement?.localName === "ol") {
        let value = Number(node.parentElement.getAttribute("start") || 1);
        for (const sibling of node.parentElement.children) {
          if (sibling.hasAttribute("value")) value = Number(sibling.getAttribute("value"));
          if (sibling === node) break;
          value++;
        }
        copy.setAttribute("value", String(value));
      }
    }
    for (const child of node.childNodes) copy.append(clean(child));
    return copy;
  }

  function newPage() {
    const slot = document.createElement("section");
    slot.className = "page-slot";
    slot.setAttribute("aria-label", `Page ${pages.length + 1}`);
    const paper = document.createElement("div");
    paper.className = "page";
    const content = document.createElement("div");
    content.className = "paper-content";
    const number = document.createElement("div");
    number.className = "page-number";
    number.setAttribute("aria-hidden", "true");
    number.textContent = String(pages.length + 1);
    paper.append(content, number);
    slot.append(paper);
    pagesRoot.append(slot);
    pages.push({ slot, paper, content });
    currentContent = content;
    parentClones = new Map();
  }

  function targetFor(parents) {
    let target = currentContent;
    for (const parent of parents) {
      if (!parentClones.has(parent)) {
        const copy = parent.cloneNode(false);
        if (continuedParents.has(parent)) copy.classList.add("continued");
        continuedParents.add(parent);
        target.append(copy);
        parentClones.set(parent, copy);
      }
      target = parentClones.get(parent);
    }
    return target;
  }

  function overflows() { return currentContent.scrollHeight > CONTENT_HEIGHT + 1; }

  // Split oversized blocks at word boundaries, carrying their semantic parents
  // across pages. Ordinary paragraphs, list items, table rows and images stay whole.
  function place(node, parents = []) {
    let target = targetFor(parents);
    const hadContent = currentContent.textContent.trim().length > 0 || !!currentContent.querySelector("img");
    target.append(node);
    if (!overflows()) return;
    const height = node instanceof Element ? node.getBoundingClientRect().height : Infinity;
    node.remove();
    if (hadContent && height < CONTENT_HEIGHT - 60) {
      newPage();
      target = targetFor(parents);
      target.append(node);
      if (!overflows()) return;
      node.remove();
    }
    if (node.nodeType === Node.TEXT_NODE) {
      const text = node.textContent;
      target.append(node);
      let low = 0;
      let high = text.length;
      while (low < high) {
        const middle = Math.ceil((low + high) / 2);
        node.textContent = text.slice(0, middle);
        if (overflows()) high = middle - 1;
        else low = middle;
      }
      let end = low;
      const boundary = text.slice(0, end).search(/\s+\S*$/);
      if (boundary > end / 2) end = boundary + 1;
      if (!end && !hadContent) end = Math.max(1, low); // Always make progress.
      node.textContent = text.slice(0, end);
      newPage();
      if (end < text.length) {
        const link = textLinks.get(node);
        place(link ? linkedText(text.slice(end), link.source, link.start + end) : document.createTextNode(text.slice(end)), parents);
      }
      return;
    }
    const atomic = node.matches("img, math, hr, br");
    if (!atomic && node.childNodes.length) {
      for (const child of [...node.childNodes]) place(child, [...parents, node]);
    } else {
      if (hadContent) { newPage(); target = targetFor(parents); }
      target.append(node);
      // A single unusually tall equation or image must still fit on its page.
      if (overflows()) {
        const ratio = (CONTENT_HEIGHT - 40) / Math.max(node.getBoundingClientRect().height, CONTENT_HEIGHT);
        node.style.zoom = String(ratio);
      }
    }
  }

  async function render() {
    const blocks = [];
    const heading = document.createElement("h1");
    heading.textContent = title;
    blocks.push({ node: heading, message: -1 });
    messages.forEach(({ role, roots }, index) => {
      const user = role === "user";
      for (const root of roots) {
        const cleaned = clean(root);
        if (user) {
          const prompt = document.createElement("div");
          prompt.className = "prompt";
          prompt.append(cleaned);
          blocks.push({ node: prompt, message: index });
        } else if (cleaned instanceof Element && cleaned.matches("p, h1, h2, h3, h4, h5, h6, ul, ol, blockquote, pre, table, img, math, hr, .source-whitespace")) {
          blocks.push({ node: cleaned, message: index });
        } else {
          for (const node of [...cleaned.childNodes]) blocks.push({ node, message: index });
        }
      }
    });
    if (!messages.length) {
      const empty = document.createElement("p");
      empty.textContent = "No readable messages were found on this page. Close the reader and try again after the conversation finishes loading.";
      blocks.push({ node: empty, message: -1 });
    }
    openingAnchor = captureAnchor([...sourceFragments.keys()], sourceBounds());
    const bookmark = reusableBookmark();
    if (bookmark) {
      scale = bookmark.scale;
      fitWidth = bookmark.fitWidth;
      outline.hidden = bookmark.outlineHidden;
      dialog.querySelector('[data-action="outline"]').setAttribute("aria-expanded", String(!outline.hidden));
    }
    dialog.showModal();
    const images = blocks.flatMap(({ node }) => node instanceof Element ? [...(node.matches("img") ? [node] : []), ...node.querySelectorAll("img")] : []);
    await Promise.race([
      Promise.allSettled(images.map((img) => img.decode())),
      new Promise((resolve) => setTimeout(resolve, 1500))
    ]);
    if (closed) return;
    pagesRoot.replaceChildren();
    newPage();
    for (let index = 0; index < blocks.length; index++) {
      const { node } = blocks[index];
      if (!node.textContent.trim() && !(node instanceof Element &&
        (node.matches("img, math, hr, br") || node.querySelector("img, math, hr, br")))) continue;
      const before = pages.length - 1;
      const label = node instanceof Element && node.matches("h1, h2, h3") ? node.textContent : null;
      place(node);
      const page = node.isConnected ? pages.findIndex(({ content }) => content.contains(node)) : before;
      if (label) {
        const item = document.createElement("button");
        item.textContent = label;
        item.addEventListener("click", () => goToPage(page));
        outline.append(item);
      }
      if (index % 30 === 29) {
        await new Promise(requestAnimationFrame);
        if (closed) return;
      }
    }
    shadow.querySelector(".page-total").textContent = `/ ${pages.length}`;
    updateZoom();
    if (bookmark) viewport.scrollTop = bookmark.readerTop;
    else restoreReaderAnchor(openingAnchor);
    initialReaderTop = viewport.scrollTop;
    initialScale = scale;
    rendered = true;
    resizeObserver.observe(viewport);
    viewport.focus({ preventScroll: true });
  }

  render().catch((error) => {
    console.warn("PDF reader rendering failed:", error);
    pagesRoot.replaceChildren();
    const status = document.createElement("div");
    status.className = "status";
    status.textContent = "Could not prepare this conversation. Press Escape to return to the chat.";
    pagesRoot.append(status);
    if (!dialog.open && !closed) dialog.showModal();
  });
})();
