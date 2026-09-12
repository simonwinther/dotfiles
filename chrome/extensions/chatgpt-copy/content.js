(() => {
  if (window.__chatgptMarkdownLatexCopyInstalled) {
    return;
  }

  window.__chatgptMarkdownLatexCopyInstalled = true;

  const ZERO_WIDTH_RE = /[\u200b\u200c\u200d\ufeff\u2060]/g;
  const LITERAL_PLACEHOLDER_PREFIX = "@@LATEX_COPY_LITERAL_";
  const COPY_BUTTON_SELECTOR =
    'button[data-testid="copy-turn-action-button"], button[aria-label="Copy message"]';
  const TURN_SELECTOR = 'section[data-turn], [data-testid^="conversation-turn"]';
  const MESSAGE_ROOT_SELECTORS = [
    '[data-message-author-role="assistant"] .markdown',
    ".markdown",
    ".whitespace-pre-wrap",
    "[data-message-author-role]"
  ];
  const MATH_SOURCE_SELECTOR = "[data-math-source]";
  const MATH_WRAPPER_SELECTOR = `${MATH_SOURCE_SELECTOR}, .katex-display, .katex`;
  const DISPLAY_MATH_SELECTOR = '.katex-display, math[display="block"]';
  const TEX_ANNOTATION_SELECTOR = 'annotation[encoding="application/x-tex"]';
  const EDITABLE_SELECTOR = 'input, textarea, [contenteditable=""], [contenteditable="true"]';
  const SECTION_COMMANDS = ["section", "subsection", "subsubsection", "paragraph", "subparagraph"];
  const SKIP_TAGS = new Set([
    "script",
    "style",
    "noscript",
    "template",
    "svg",
    "canvas",
    "button",
    "input",
    "textarea",
    "select",
    "option"
  ]);
  const SKIP_CLASSES = ["katex-html", "katex-mathml", "sr-only"];
  const BLOCK_TAGS = new Set([
    "address",
    "article",
    "aside",
    "blockquote",
    "dd",
    "details",
    "dialog",
    "div",
    "dl",
    "dt",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "header",
    "li",
    "main",
    "nav",
    "ol",
    "p",
    "pre",
    "section",
    "table",
    "ul"
  ]);
  let internalCopy = false;
  let toastTimer = null;
  let enabled = true;
  let outputFormat = "latex";

  initSettings();

  document.addEventListener("copy", handleCopy, true);
  document.addEventListener("click", handleCopyButtonClick, true);

  function initSettings() {
    try {
      chrome.storage.local.get({ enabled: true, format: "latex" }, (items) => {
        if (!chrome.runtime.lastError) {
          enabled = items.enabled !== false;
          outputFormat = items.format === "markdown" ? "markdown" : "latex";
        }
      });

      chrome.storage.onChanged.addListener((changes, area) => {
        if (area !== "local") {
          return;
        }

        if (changes.enabled) {
          enabled = changes.enabled.newValue !== false;
        }
        if (changes.format) {
          outputFormat = changes.format.newValue === "markdown" ? "markdown" : "latex";
        }
      });
    } catch {
      // chrome.storage is unavailable (e.g. orphaned script after an
      // extension reload); keep the last known state.
    }
  }

  function handleCopy(event) {
    if (!enabled || internalCopy || !event.clipboardData) {
      return;
    }

    const selection = document.getSelection();
    if (!selection || selection.isCollapsed || selection.rangeCount === 0) {
      return;
    }

    if (isEditableSelection(selection)) {
      return;
    }

    const context = createCopyContext();
    const text = textFromSelection(selection, context);
    if (!text) {
      return;
    }

    event.preventDefault();
    event.stopImmediatePropagation();
    event.clipboardData.clearData();
    event.clipboardData.setData("text/plain", text);
    event.clipboardData.setData(context.format === "latex" ? "text/x-tex" : "text/markdown", text);
    showToast(copySuccessMessage(context));
  }

  function handleCopyButtonClick(event) {
    if (!enabled) {
      return;
    }

    const target = event.target instanceof Element ? event.target : null;
    const button = target?.closest(COPY_BUTTON_SELECTOR);
    if (!button) {
      return;
    }

    const messageRoot = findMessageRoot(button);
    if (!messageRoot) {
      return;
    }

    const context = createCopyContext();
    const text = normalizeText(nodeToText(messageRoot, context), context);
    if (!text) {
      return;
    }

    event.preventDefault();
    event.stopImmediatePropagation();
    writeClipboardText(text)
      .then(() => showToast(copySuccessMessage(context)))
      .catch(() => showToast("Could not write clipboard"));
  }

  function createCopyContext() {
    return { format: outputFormat, literals: [] };
  }

  function copySuccessMessage(context) {
    return context.format === "latex" ? "Copied as LaTeX" : "Copied as Markdown";
  }

  function textFromSelection(selection, context) {
    const container = document.createElement("div");

    for (let index = 0; index < selection.rangeCount; index += 1) {
      const range = selection.getRangeAt(index);
      if (range.collapsed) {
        continue;
      }

      const copyRange = range.cloneRange();
      expandRangeAroundMath(copyRange);
      container.appendChild(cloneSelectionContents(copyRange));
      if (index < selection.rangeCount - 1) {
        container.appendChild(document.createTextNode("\n\n"));
      }
    }

    return normalizeText(childrenToText(container, context), context);
  }

  function cloneSelectionContents(range) {
    let contents = range.cloneContents();
    let ancestor = range.commonAncestorContainer;
    if (!(ancestor instanceof Element)) {
      ancestor = ancestor.parentElement;
    }

    while (ancestor && !ancestor.matches(".markdown, [data-message-author-role], body, html")) {
      if (ancestor.matches("strong, b, em, i, s, del, sub, sup, a, code, pre")) {
        const wrapper = ancestor.cloneNode(false);
        wrapper.appendChild(contents);
        contents = wrapper;
      }
      ancestor = ancestor.parentElement;
    }

    return contents;
  }

  function findMessageRoot(button) {
    const turn = button.closest(TURN_SELECTOR);
    if (!turn) {
      return null;
    }

    for (const selector of MESSAGE_ROOT_SELECTORS) {
      const messageRoot = turn.querySelector(selector);
      if (messageRoot) {
        return messageRoot;
      }
    }

    return turn;
  }

  function expandRangeAroundMath(range) {
    const startMath = closestMathWrapper(range.startContainer);
    const endMath = closestMathWrapper(range.endContainer);

    if (startMath?.parentNode) {
      range.setStartBefore(startMath);
    }

    if (endMath?.parentNode) {
      range.setEndAfter(endMath);
    }
  }

  function closestMathWrapper(node) {
    const element = node instanceof Element ? node : node?.parentElement;
    let math = element?.closest(MATH_WRAPPER_SELECTOR);
    if (!math) {
      return null;
    }

    let parentMath = math.parentElement?.closest(MATH_WRAPPER_SELECTOR);
    while (parentMath) {
      math = parentMath;
      parentMath = math.parentElement?.closest(MATH_WRAPPER_SELECTOR);
    }

    return math;
  }

  function nodeToText(node, context = {}) {
    if (node.nodeType === Node.TEXT_NODE) {
      return textNodeToText(node, context);
    }

    if (node.nodeType !== Node.ELEMENT_NODE) {
      return "";
    }

    const element = node;
    if (isMathElement(element)) {
      return mathToText(element, context);
    }

    if (shouldSkipElement(element)) {
      return "";
    }

    const tag = element.tagName.toLowerCase();

    if (element.classList.contains("whitespace-pre-wrap") && element.children.length === 0) {
      return asBlock(formatPlainText(cleanPlainText(element.textContent, true), context));
    }

    switch (tag) {
      case "br":
        return context.format === "latex" && !context.inTable ? "\\\\\n" : "\n";
      case "hr":
        return asBlock(context.format === "latex" ? "\\noindent\\rule{\\linewidth}{0.4pt}" : "---");
      case "p":
        return asBlock(childrenToText(element, context));
      case "h1":
      case "h2":
      case "h3":
      case "h4":
      case "h5":
      case "h6":
        return headingToText(element, context);
      case "strong":
      case "b":
        return formatInline("**", "textbf", childrenToText(element, context), context);
      case "em":
      case "i":
        return formatInline("*", "emph", childrenToText(element, context), context);
      case "s":
      case "del":
        return context.format === "latex"
          ? childrenToText(element, context)
          : wrapInline("~~", childrenToText(element, context));
      case "sub":
      case "sup": {
        const text = childrenToText(element, context);
        return context.format === "latex"
          ? `\\text${tag === "sub" ? "subscript" : "superscript"}{${text}}`
          : `<${tag}>${text}</${tag}>`;
      }
      case "code":
        return codeToText(element, context);
      case "pre":
        return preToText(element, context);
      case "ul":
        return listToText(element, false, context);
      case "ol":
        return listToText(element, true, context);
      case "blockquote":
        return blockquoteToText(element, context);
      case "a":
        return linkToText(element, context);
      case "table":
        return tableToText(element, context);
      case "img":
        return imageToText(element, context);
      case "section":
      case "article":
      case "main":
        return childrenToText(element, context);
      case "div":
        return divToText(element, context);
      default:
        if (isBlockTag(tag)) {
          return asBlock(childrenToText(element, context));
        }
        return childrenToText(element, context);
    }
  }

  function childrenToText(element, context = {}) {
    let text = "";
    for (const child of element.childNodes) {
      text += nodeToText(child, context);
    }

    return text;
  }

  function textNodeToText(node, context) {
    if (context.pre) {
      return node.nodeValue || "";
    }

    return formatPlainText(cleanPlainText(node.nodeValue || "", false), context);
  }

  function formatPlainText(text, context) {
    return context.format === "latex" ? escapeLatexText(text) : text;
  }

  function preserveLiteral(text, context) {
    const placeholder = `${LITERAL_PLACEHOLDER_PREFIX}${context.literals.length}@@`;
    context.literals.push(text);
    return placeholder;
  }

  function cleanPlainText(text, preserveLineBreaks) {
    let value = text.replace(ZERO_WIDTH_RE, "").replace(/\u00a0/g, " ");
    if (preserveLineBreaks) {
      value = value
        .replace(/\r\n?/g, "\n")
        .replace(/[ \t]+\n/g, "\n")
        .replace(/\n[ \t]+/g, "\n");
      return value;
    }

    return value.replace(/\s+/g, " ");
  }

  function isMathElement(element) {
    if (element.matches(MATH_WRAPPER_SELECTOR)) {
      return true;
    }

    return element.tagName.toLowerCase() === "math" && Boolean(findTexAnnotation(element));
  }

  function mathToText(element, context) {
    const source = element.matches(MATH_SOURCE_SELECTOR)
      ? element
      : element.querySelector(MATH_SOURCE_SELECTOR);
    const tex =
      source?.getAttribute("data-math-source")?.trim() ||
      findTexAnnotation(element)?.textContent?.trim();
    if (!tex) {
      return formatPlainText(cleanPlainText(element.textContent || "", false), context);
    }

    const isDisplay =
      element.matches(DISPLAY_MATH_SELECTOR) ||
      element.getAttribute("display") === "block" ||
      source?.style.display === "block" ||
      element.querySelector(DISPLAY_MATH_SELECTOR) !== null;

    if (isDisplay && !context.inTable) {
      const math = context.format === "latex" ? `\\[\n${tex}\n\\]` : `$$\n${tex}\n$$`;
      return asBlock(preserveLiteral(math, context));
    }

    const math = context.format === "latex" ? `\\(${tex}\\)` : `$${tex}$`;
    return preserveLiteral(math, context);
  }

  function findTexAnnotation(element) {
    return element.querySelector(TEX_ANNOTATION_SELECTOR);
  }

  function shouldSkipElement(element) {
    const tag = element.tagName.toLowerCase();
    if (SKIP_TAGS.has(tag)) {
      return true;
    }

    for (const className of SKIP_CLASSES) {
      if (element.classList.contains(className)) {
        return true;
      }
    }

    if (element.hasAttribute("data-copy-ignore")) {
      return true;
    }

    if (element.hasAttribute("hidden")) {
      return true;
    }

    if (element.getAttribute("aria-hidden") === "true" && !containsMath(element)) {
      return true;
    }

    return false;
  }

  function containsMath(element) {
    return Boolean(element.querySelector?.(`${MATH_WRAPPER_SELECTOR}, ${TEX_ANNOTATION_SELECTOR}`));
  }

  function headingToText(element, context) {
    const level = Number(element.tagName.slice(1));
    const text = childrenToText(element, context).trim();
    if (!text) {
      return "";
    }

    if (context.format === "latex") {
      const sectionCommand = SECTION_COMMANDS[level - 1] || "textbf";
      return asBlock(`\\${sectionCommand}{${stripHeadingNumber(text)}}`);
    }

    return `\n\n${"#".repeat(level)} ${text}\n\n`;
  }

  function stripHeadingNumber(text) {
    return text.replace(/^\s*\d+(?:\.\d+)*[.)]\s*/, "");
  }

  function escapeLatexText(text) {
    return text.replace(/[\\#$%&_{}~^]/g, escapeLatexTextChar);
  }

  function escapeLatexTextChar(char) {
    switch (char) {
      case "\\":
        return "\\textbackslash{}";
      case "~":
        return "\\textasciitilde{}";
      case "^":
        return "\\textasciicircum{}";
      default:
        return `\\${char}`;
    }
  }

  function wrapInline(marker, value) {
    const text = value.trim();
    return text ? `${marker}${text}${marker}` : "";
  }

  function formatInline(marker, command, value, context) {
    const text = value.trim();
    if (!text) {
      return "";
    }

    return context.format === "latex" ? `\\${command}{${text}}` : wrapInline(marker, text);
  }

  function codeToText(element, context) {
    if (element.closest("pre")) {
      return "";
    }

    const text = cleanPlainText(element.textContent || "", false).trim();
    if (!text) {
      return "";
    }

    const code = context.format === "latex" ? `\\texttt{${escapeLatexText(text)}}` : inlineCode(text);
    return preserveLiteral(code, context);
  }

  function inlineCode(text) {
    if (!text) {
      return "";
    }

    const longestRun = longestBacktickRun(text);
    const fence = "`".repeat(Math.max(1, longestRun + 1));
    const needsPadding = text.startsWith("`") || text.endsWith("`");
    const body = needsPadding ? ` ${text} ` : text;
    return `${fence}${body}${fence}`;
  }

  function preToText(element, context) {
    const code = element.querySelector("code") || element;
    const text = (code.textContent || "").replace(/\n+$/g, "");
    if (!text) {
      return "";
    }

    if (context.format === "latex") {
      let block = `\\begin{verbatim}\n${text}\n\\end{verbatim}`;
      if (context.inTable) {
        const lines = text.split("\n").map((line) => `\\texttt{${escapeLatexText(line)}}`);
        block = `\\shortstack[l]{${lines.join(" \\\\ ")}}`;
      }
      return asBlock(preserveLiteral(block, context));
    }

    const language = detectLanguage(element, code);
    const fence = "`".repeat(Math.max(3, longestBacktickRun(text) + 1));
    return asBlock(preserveLiteral(`${fence}${language}\n${text}\n${fence}`, context));
  }

  function detectLanguage(pre, code) {
    const candidates = [
      pre.getAttribute("data-language"),
      code.getAttribute("data-language"),
      ...Array.from(pre.classList),
      ...Array.from(code.classList)
    ];

    for (const candidate of candidates) {
      if (!candidate) {
        continue;
      }

      const language = candidate.replace(/^language-/, "").replace(/^lang-/, "");
      if (/^[a-z0-9_+#.-]+$/i.test(language) && !["hljs", "code"].includes(language)) {
        return language;
      }
    }

    return "";
  }

  function longestBacktickRun(text) {
    const runs = text.match(/`+/g) || [];
    return runs.reduce((longest, run) => Math.max(longest, run.length), 0);
  }

  function listToText(element, ordered, context) {
    const items = Array.from(element.children).filter(
      (child) => child.tagName?.toLowerCase() === "li"
    );
    if (items.length === 0) {
      return "";
    }

    const start = ordered ? Number(element.getAttribute("start") || "1") : 1;
    if (context.format === "latex") {
      const environment = ordered ? "enumerate" : "itemize";
      const depth = context.enumerateDepth || 0;
      const counter = ["enumi", "enumii", "enumiii", "enumiv"][depth];
      const numbering = ordered && counter && Number.isInteger(start) && start !== 1
        ? `\\setcounter{${counter}}{${start - 1}}\n`
        : "";
      const lines = items.map((item) => {
        const content = childrenToText(item, {
          ...context,
          inList: true,
          enumerateDepth: depth + (ordered ? 1 : 0)
        }).trim();
        return `\\item ${content.startsWith("[") ? "{}" : ""}${content}`;
      });

      return asBlock(`\\begin{${environment}}\n${numbering}${lines.join("\n")}\n\\end{${environment}}`);
    }

    const lines = items.map((item, index) => {
      const marker = ordered ? `${start + index}. ` : "- ";
      const content = normalizeText(childrenToText(item, { ...context, inList: true }), context);
      return formatListItem(marker, content);
    });

    return asBlock(preserveLiteral(lines.join("\n"), context));
  }

  function formatListItem(marker, content) {
    if (!content) {
      return marker.trimEnd();
    }

    const indent = " ".repeat(marker.length);
    const lines = content.split("\n");
    return [marker + lines[0], ...lines.slice(1).map((line) => (line ? indent + line : ""))].join(
      "\n"
    );
  }

  function blockquoteToText(element, context) {
    const text = normalizeText(childrenToText(element, context), context);
    if (!text) {
      return "";
    }

    if (context.format === "latex") {
      return asBlock(preserveLiteral(`\\begin{quote}\n${text}\n\\end{quote}`, context));
    }

    const quoted = text
      .split("\n")
      .map((line) => (line ? `> ${line}` : ">"))
      .join("\n");
    return asBlock(preserveLiteral(quoted, context));
  }

  function linkToText(element, context) {
    const text = childrenToText(element, context).trim();
    const href = element.href || element.getAttribute("href") || "";
    if (!href || href.startsWith("javascript:")) {
      return text;
    }

    if (context.format === "latex") {
      const url = `\\texttt{${escapeLatexText(href)}}`;
      return !text || text === escapeLatexText(href) ? url : `${text} (${url})`;
    }

    if (!text || text === href) {
      return href;
    }

    return `[${text.replace(/[[\]]/g, "\\$&")}](${href.replace(/\)/g, "%29")})`;
  }

  function tableToText(element, context) {
    const rows = Array.from(element.querySelectorAll("tr"))
      .map((row) => Array.from(row.children).filter((cell) => /^(td|th)$/i.test(cell.tagName)))
      .filter((cells) => cells.length > 0);

    if (rows.length === 0) {
      return "";
    }

    const textRows = rows.map((cells) =>
      cells.map((cell) => tableCellToText(cell, context))
    );
    const columnCount = Math.max(...textRows.map((row) => row.length));
    const normalizedRows = textRows.map((row) => padRow(row, columnCount));
    if (context.format === "latex") {
      const columns = Array.from({ length: columnCount }, () => "l").join(" ");
      const lines = normalizedRows.map((row) => `${row.join(" & ")} \\\\`);
      if (rows[0].some((cell) => cell.tagName.toLowerCase() === "th")) {
        lines.splice(1, 0, "\\hline");
      }
      return asBlock(`\\begin{tabular}{${columns}}\n${lines.join("\n")}\n\\end{tabular}`);
    }

    const header = normalizedRows[0];
    const body = normalizedRows.slice(1);
    const separator = Array.from({ length: columnCount }, () => "---");
    const lines = [header, separator, ...body].map((row) => `| ${row.join(" | ")} |`);

    return `\n\n${lines.join("\n")}\n\n`;
  }

  function tableCellToText(cell, context) {
    const text = childrenToText(cell, { ...context, inTable: true }).trim();
    if (context.format === "latex") {
      const lines = text.split(/\n+/);
      return lines.length > 1 ? `\\shortstack[l]{${lines.join(" \\\\ ")}}` : text;
    }

    return normalizeText(text, context)
      .replace(/\n+/g, "<br>")
      .replace(/\|/g, "\\|")
      .trim();
  }

  function padRow(row, length) {
    return [...row, ...Array.from({ length: Math.max(0, length - row.length) }, () => "")];
  }

  function imageToText(element, context) {
    const alt = element.getAttribute("alt")?.trim();
    if (!alt) {
      return "";
    }

    return context.format === "latex" ? `\\textit{Image: ${escapeLatexText(alt)}}` : `[image: ${alt}]`;
  }

  function divToText(element, context) {
    const text = childrenToText(element, context);
    if (!text.trim()) {
      return "";
    }

    if (element.matches(".markdown, .text-message, [data-message-author-role]")) {
      return text;
    }

    if (hasDirectBlockChild(element)) {
      return text;
    }

    return asBlock(text);
  }

  function hasDirectBlockChild(element) {
    for (const child of element.children) {
      if (isBlockTag(child.tagName.toLowerCase())) {
        return true;
      }
    }

    return false;
  }

  function isBlockTag(tag) {
    return BLOCK_TAGS.has(tag);
  }

  function asBlock(value) {
    const text = value.trim();
    return text ? `\n\n${text}\n\n` : "";
  }

  function normalizeText(value, context) {
    const text = value
      .replace(ZERO_WIDTH_RE, "")
      .replace(/\u00a0/g, " ")
      .replace(/[ \t]+\n/g, "\n")
      .replace(/\n{3,}/g, "\n\n")
      .split("\n")
      .map((line) => line.replace(/[ \t]+$/g, ""))
      .join("\n")
      .trim();

    return text.replace(/@@LATEX_COPY_LITERAL_(\d+)@@/g, (placeholder, index) =>
      context.literals[Number(index)] ?? placeholder
    );
  }

  function isEditableSelection(selection) {
    const anchor = selection.anchorNode;
    const focus = selection.focusNode;
    return isNodeInEditable(anchor) || isNodeInEditable(focus);
  }

  function isNodeInEditable(node) {
    const element = node instanceof Element ? node : node?.parentElement;
    return Boolean(element?.closest(EDITABLE_SELECTOR));
  }

  async function writeClipboardText(text) {
    if (navigator.clipboard?.writeText) {
      try {
        await navigator.clipboard.writeText(text);
        return;
      } catch {
        // Fall back to execCommand when clipboardWrite is unavailable in-page.
      }
    }

    fallbackCopyText(text);
  }

  function fallbackCopyText(text) {
    const textarea = document.createElement("textarea");
    textarea.value = text;
    textarea.setAttribute("readonly", "");
    textarea.style.position = "fixed";
    textarea.style.top = "-1000px";
    textarea.style.left = "-1000px";
    document.documentElement.appendChild(textarea);

    try {
      internalCopy = true;
      textarea.select();
      document.execCommand("copy");
    } finally {
      internalCopy = false;
      textarea.remove();
    }
  }

  function showToast(message) {
    let toast = document.getElementById("chatgpt-markdown-latex-copy-toast");
    if (!toast) {
      toast = document.createElement("div");
      toast.id = "chatgpt-markdown-latex-copy-toast";
      toast.setAttribute("data-copy-ignore", "true");
      toast.style.position = "fixed";
      toast.style.right = "16px";
      toast.style.bottom = "16px";
      toast.style.zIndex = "2147483647";
      toast.style.padding = "8px 10px";
      toast.style.borderRadius = "8px";
      toast.style.background = "rgba(24, 24, 27, 0.92)";
      toast.style.color = "#fff";
      toast.style.font = "12px system-ui, -apple-system, BlinkMacSystemFont, sans-serif";
      toast.style.boxShadow = "0 8px 30px rgba(0, 0, 0, 0.25)";
      toast.style.pointerEvents = "none";
      toast.style.opacity = "0";
      toast.style.transition = "opacity 120ms ease";
      document.documentElement.appendChild(toast);
    }

    toast.textContent = message;
    toast.style.opacity = "1";
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => {
      toast.style.opacity = "0";
    }, 1400);
  }
})();
