(() => {
  if (window.__chatgptMarkdownLatexCopyInstalled) {
    return;
  }

  window.__chatgptMarkdownLatexCopyInstalled = true;

  const ZERO_WIDTH_RE = /[\u200b\u200c\u200d\ufeff\u2060]/g;
  const CODE_PLACEHOLDER_PREFIX = "@@CHATGPT_LATEX_COPY_CODE_";
  const COPY_BUTTON_SELECTOR =
    'button[data-testid="copy-turn-action-button"], button[aria-label="Copy message"]';
  const TURN_SELECTOR = 'section[data-turn], [data-testid^="conversation-turn"]';
  const MESSAGE_ROOT_SELECTORS = [
    '[data-message-author-role="assistant"] .markdown',
    ".markdown",
    ".whitespace-pre-wrap",
    "[data-message-author-role]"
  ];
  const MATH_WRAPPER_SELECTOR = ".katex-display, .katex";
  const TEX_ANNOTATION_SELECTOR = 'annotation[encoding="application/x-tex"]';
  const EDITABLE_SELECTOR = 'input, textarea, [contenteditable=""], [contenteditable="true"]';
  const INLINE_MATH_RE = /(\\\([\s\S]*?\\\)|\\\[[\s\S]*?\\\])/g;
  const SECTION_COMMANDS = ["section", "subsection", "subsubsection"];
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

  document.addEventListener("copy", handleCopy, true);
  document.addEventListener("click", handleCopyButtonClick, true);

  function handleCopy(event) {
    if (internalCopy || !event.clipboardData) {
      return;
    }

    const selection = document.getSelection();
    if (!selection || selection.isCollapsed || selection.rangeCount === 0) {
      return;
    }

    if (isEditableSelection(selection)) {
      return;
    }

    const markdown = markdownFromSelection(selection);
    if (!markdown) {
      return;
    }

    event.preventDefault();
    event.stopImmediatePropagation();
    event.clipboardData.setData("text/plain", markdown);
    event.clipboardData.setData("text/markdown", markdown);
    showToast("Copied as Markdown + LaTeX");
  }

  function handleCopyButtonClick(event) {
    const target = event.target instanceof Element ? event.target : null;
    const button = target?.closest(COPY_BUTTON_SELECTOR);
    if (!button) {
      return;
    }

    const messageRoot = findMessageRoot(button);
    if (!messageRoot) {
      return;
    }

    const markdown = normalizeMarkdown(nodeToMarkdown(messageRoot));
    if (!markdown) {
      return;
    }

    event.preventDefault();
    event.stopImmediatePropagation();
    writeClipboardText(markdown)
      .then(() => showToast("Copied as Markdown + LaTeX"))
      .catch(() => showToast("Could not write clipboard"));
  }

  function markdownFromSelection(selection) {
    const container = document.createElement("div");

    for (let index = 0; index < selection.rangeCount; index += 1) {
      const range = selection.getRangeAt(index);
      if (range.collapsed) {
        continue;
      }

      const copyRange = range.cloneRange();
      expandRangeAroundMath(copyRange);
      container.appendChild(copyRange.cloneContents());
      if (index < selection.rangeCount - 1) {
        container.appendChild(document.createTextNode("\n\n"));
      }
    }

    return normalizeMarkdown(childrenToMarkdown(container));
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
    const math = element?.closest(MATH_WRAPPER_SELECTOR);
    if (!math) {
      return null;
    }

    return math.closest(".katex-display") || math;
  }

  function nodeToMarkdown(node, context = {}) {
    if (node.nodeType === Node.TEXT_NODE) {
      return textNodeToMarkdown(node, context);
    }

    if (node.nodeType !== Node.ELEMENT_NODE) {
      return "";
    }

    const element = node;
    if (isMathElement(element)) {
      return mathToMarkdown(element);
    }

    if (shouldSkipElement(element)) {
      return "";
    }

    const tag = element.tagName.toLowerCase();

    if (element.classList.contains("whitespace-pre-wrap")) {
      return asBlock(cleanPlainText(element.textContent, true));
    }

    switch (tag) {
      case "br":
        return "\n";
      case "hr":
        return "\n\n---\n\n";
      case "p":
        return asBlock(childrenToMarkdown(element, context));
      case "h1":
      case "h2":
      case "h3":
      case "h4":
      case "h5":
      case "h6":
        return headingToMarkdown(element, context);
      case "strong":
      case "b":
        return wrapInline("**", childrenToMarkdown(element, context));
      case "em":
      case "i":
        return wrapInline("*", childrenToMarkdown(element, context));
      case "s":
      case "del":
        return wrapInline("~~", childrenToMarkdown(element, context));
      case "code":
        return codeToMarkdown(element);
      case "pre":
        return preToMarkdown(element);
      case "ul":
        return listToMarkdown(element, false, context);
      case "ol":
        return listToMarkdown(element, true, context);
      case "blockquote":
        return blockquoteToMarkdown(element, context);
      case "a":
        return linkToMarkdown(element, context);
      case "table":
        return tableToMarkdown(element, context);
      case "img":
        return imageToMarkdown(element);
      case "section":
      case "article":
      case "main":
        return childrenToMarkdown(element, context);
      case "div":
        return divToMarkdown(element, context);
      default:
        if (isBlockTag(tag)) {
          return asBlock(childrenToMarkdown(element, context));
        }
        return childrenToMarkdown(element, context);
    }
  }

  function childrenToMarkdown(element, context = {}) {
    let markdown = "";
    for (const child of element.childNodes) {
      markdown += nodeToMarkdown(child, context);
    }

    return markdown;
  }

  function textNodeToMarkdown(node, context) {
    if (context.pre) {
      return node.nodeValue || "";
    }

    return cleanPlainText(node.nodeValue || "", false);
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
    if (element.classList.contains("katex-display") || element.classList.contains("katex")) {
      return true;
    }

    return element.tagName.toLowerCase() === "math" && Boolean(findTexAnnotation(element));
  }

  function mathToMarkdown(element) {
    const annotation = findTexAnnotation(element);
    const tex = annotation?.textContent?.trim();
    if (!tex) {
      return cleanPlainText(element.textContent || "", false);
    }

    const isDisplay =
      element.classList.contains("katex-display") ||
      element.getAttribute("display") === "block" ||
      element.querySelector('math[display="block"]') !== null;

    if (isDisplay) {
      return `\n\n\\[\n${tex}\n\\]\n\n`;
    }

    return `\\(${tex}\\)`;
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

  function headingToMarkdown(element, context) {
    const level = Number(element.tagName.slice(1));
    const text = childrenToMarkdown(element, context).trim();
    if (!text) {
      return "";
    }

    const sectionCommand = SECTION_COMMANDS[level - 1];
    if (sectionCommand) {
      return `\n\n\\${sectionCommand}{${escapeLatexSectionTitle(stripHeadingNumber(text))}}\n\n`;
    }

    return `\n\n${"#".repeat(level)} ${text}\n\n`;
  }

  function stripHeadingNumber(text) {
    return text.replace(/^\s*\d+(?:\.\d+)*[.)]\s*/, "");
  }

  function escapeLatexSectionTitle(text) {
    return text
      .split(INLINE_MATH_RE)
      .map((part) => {
        if (part.startsWith("\\(") || part.startsWith("\\[")) {
          return part;
        }

        return part.replace(/[\\#$%&_{}~^]/g, escapeLatexTextChar);
      })
      .join("");
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

  function codeToMarkdown(element) {
    if (element.closest("pre")) {
      return "";
    }

    return inlineCode(cleanPlainText(element.textContent || "", false).trim());
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

  function preToMarkdown(element) {
    const code = element.querySelector("code") || element;
    const text = (code.textContent || "").replace(/\n+$/g, "");
    if (!text) {
      return "";
    }

    const language = detectLanguage(element, code);
    const fence = "`".repeat(Math.max(3, longestBacktickRun(text) + 1));
    return `\n\n${fence}${language}\n${text}\n${fence}\n\n`;
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

  function listToMarkdown(element, ordered, context) {
    const items = Array.from(element.children).filter(
      (child) => child.tagName?.toLowerCase() === "li"
    );
    if (items.length === 0) {
      return "";
    }

    const start = ordered ? Number(element.getAttribute("start") || "1") : 1;
    const lines = items.map((item, index) => {
      const marker = ordered ? `${start + index}. ` : "- ";
      const content = childrenToMarkdown(item, { ...context, inList: true }).trim();
      return formatListItem(marker, content);
    });

    return `\n\n${lines.join("\n")}\n\n`;
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

  function blockquoteToMarkdown(element, context) {
    const text = childrenToMarkdown(element, context).trim();
    if (!text) {
      return "";
    }

    const quoted = text
      .split("\n")
      .map((line) => (line ? `> ${line}` : ">"))
      .join("\n");
    return `\n\n${quoted}\n\n`;
  }

  function linkToMarkdown(element, context) {
    const text = childrenToMarkdown(element, context).trim();
    const href = element.href || element.getAttribute("href") || "";
    if (!href || href.startsWith("javascript:")) {
      return text;
    }

    if (!text || text === href) {
      return href;
    }

    return `[${text.replace(/[[\]]/g, "\\$&")}](${href.replace(/\)/g, "%29")})`;
  }

  function tableToMarkdown(element, context) {
    const rows = Array.from(element.querySelectorAll("tr"))
      .map((row) => Array.from(row.children).filter((cell) => /^(td|th)$/i.test(cell.tagName)))
      .filter((cells) => cells.length > 0);

    if (rows.length === 0) {
      return "";
    }

    const markdownRows = rows.map((cells) =>
      cells.map((cell) => tableCellToMarkdown(cell, context))
    );
    const columnCount = Math.max(...markdownRows.map((row) => row.length));
    const normalizedRows = markdownRows.map((row) => padRow(row, columnCount));
    const header = normalizedRows[0];
    const body = normalizedRows.slice(1);
    const separator = Array.from({ length: columnCount }, () => "---");
    const lines = [header, separator, ...body].map((row) => `| ${row.join(" | ")} |`);

    return `\n\n${lines.join("\n")}\n\n`;
  }

  function tableCellToMarkdown(cell, context) {
    return childrenToMarkdown(cell, context)
      .replace(/\n+/g, "<br>")
      .replace(/\|/g, "\\|")
      .trim();
  }

  function padRow(row, length) {
    return [...row, ...Array.from({ length: Math.max(0, length - row.length) }, () => "")];
  }

  function imageToMarkdown(element) {
    const alt = element.getAttribute("alt")?.trim();
    return alt ? `[image: ${alt}]` : "";
  }

  function divToMarkdown(element, context) {
    const markdown = childrenToMarkdown(element, context);
    if (!markdown.trim()) {
      return "";
    }

    if (element.matches(".markdown, .text-message, [data-message-author-role]")) {
      return markdown;
    }

    if (hasDirectBlockChild(element)) {
      return markdown;
    }

    return asBlock(markdown);
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

  function normalizeMarkdown(markdown) {
    const codeBlocks = [];
    let text = markdown
      .replace(ZERO_WIDTH_RE, "")
      .replace(/\u00a0/g, " ")
      .replace(/[ \t]+\n/g, "\n");

    if (text.includes("```")) {
      text = text.replace(/(`{3,})[^\n]*\n[\s\S]*?\n\1/g, (block) => {
        const placeholder = `${CODE_PLACEHOLDER_PREFIX}${codeBlocks.length}@@`;
        codeBlocks.push(block);
        return placeholder;
      });
    }

    text = text
      .replace(/\n{3,}/g, "\n\n")
      .split("\n")
      .map((line) => line.replace(/[ \t]+$/g, ""))
      .join("\n")
      .trim();

    codeBlocks.forEach((block, index) => {
      text = text.replace(`${CODE_PLACEHOLDER_PREFIX}${index}@@`, block);
    });

    return text;
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
