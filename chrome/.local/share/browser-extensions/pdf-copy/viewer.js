// PDF viewer that copies selections as LaTeX or Markdown.
import { analyze, readPageData, serializeSelection } from "./lib/convert.js";
import { itemSelection, selectedLeaves } from "./lib/selection.js";

const pdfjsLib = await import("./vendor/pdfjs/build/pdf.min.mjs");
globalThis.pdfjsLib = pdfjsLib;
const { EventBus, LinkTarget, PDFFindController, PDFLinkService, PDFViewer } = await import("./vendor/pdfjs/web/pdf_viewer.mjs");
pdfjsLib.GlobalWorkerOptions.workerSrc = chrome.runtime.getURL("vendor/pdfjs/build/pdf.worker.min.mjs");

const FORMAT_NAMES = { latex: "LaTeX", markdown: "Markdown", plain: "plain text" };
const ZOOM_STEPS = 1;
const elements = Object.fromEntries([
  "title", "page-number", "page-count", "zoom-out", "zoom", "zoom-in", "fit", "rotate", "find-toggle",
  "download", "print", "open-in-chrome", "findbar", "find-input", "find-count", "find-previous",
  "find-next", "find-close", "viewerContainer", "viewer", "status", "toast"
].map((id) => [id, document.getElementById(id)]));

const settings = { enabled: true, format: "latex" };
const pageModels = new Map();
let pdfDocument = null;
let fileName = "document.pdf";
let toastTimer = 0;
let fitMode = "page-width";

const sourceUrl = (() => {
  const search = location.search;
  const index = search.indexOf("file=");
  if (index < 0) return null;
  const value = search.slice(index + 5);
  // DNR redirects insert the raw URL; navigations from this extension encode it.
  return /^[a-z][a-z0-9+.-]*%3A/i.test(value) ? decodeURIComponent(value) : value;
})();

// ---------------------------------------------------------------------------
// Viewer

const eventBus = new EventBus();
const linkService = new PDFLinkService({ eventBus, externalLinkTarget: LinkTarget.BLANK });
const findController = new PDFFindController({ eventBus, linkService });
const pdfViewer = new PDFViewer({
  container: elements.viewerContainer,
  viewer: elements.viewer,
  eventBus,
  linkService,
  findController,
  removePageBorders: true,
  textLayerMode: 1
});
linkService.setViewer(pdfViewer);

eventBus.on("pagesinit", () => {
  pdfViewer.currentScaleValue = fitMode;
  const page = Number(new URLSearchParams(location.hash.slice(1)).get("page"));
  if (page >= 1 && page <= pdfDocument.numPages) pdfViewer.currentPageNumber = page;
  elements["page-number"].value = String(pdfViewer.currentPageNumber);
});
eventBus.on("pagechanging", ({ pageNumber }) => {
  elements["page-number"].value = String(pageNumber);
});
eventBus.on("scalechanging", ({ scale }) => {
  elements.zoom.value = `${Math.round(scale * 100)}%`;
});
eventBus.on("textlayerrendered", ({ pageNumber }) => {
  // Analyze visible pages ahead of a copy so the clipboard can be filled synchronously.
  (window.requestIdleCallback || setTimeout)(() => pageModel(pageNumber).catch(() => {}));
});

function showStatus(message, detail = "", action = null) {
  elements.status.replaceChildren();
  const title = document.createElement("div");
  title.textContent = message;
  elements.status.append(title);
  if (detail) {
    const text = document.createElement("div");
    text.className = "detail";
    text.textContent = detail;
    elements.status.append(text);
  }
  if (action) {
    const button = document.createElement("button");
    button.type = "button";
    button.textContent = action.label;
    button.addEventListener("click", action.run);
    elements.status.append(button);
  }
  elements.status.hidden = false;
}

function readLocalFile(url) {
  return new Promise((resolve, reject) => {
    const request = new XMLHttpRequest();
    request.open("GET", url);
    request.responseType = "arraybuffer";
    request.onload = () => (request.response && (request.status === 0 || request.status === 200)
      ? resolve(new Uint8Array(request.response))
      : reject(new Error(`Could not read ${url}`)));
    request.onerror = () => reject(Object.assign(new Error("File access denied"), { fileAccess: true }));
    request.send();
  });
}

async function openDocument(url) {
  const params = {
    cMapUrl: chrome.runtime.getURL("vendor/pdfjs/cmaps/"),
    cMapPacked: true,
    standardFontDataUrl: chrome.runtime.getURL("vendor/pdfjs/standard_fonts/"),
    wasmUrl: chrome.runtime.getURL("vendor/pdfjs/wasm/"),
    iccUrl: chrome.runtime.getURL("vendor/pdfjs/iccs/"),
    fontExtraProperties: true,
    isEvalSupported: false,
    enableXfa: false
  };
  if (url.startsWith("file:")) {
    if (!(await chrome.extension.isAllowedFileSchemeAccess())) {
      throw Object.assign(new Error("File access denied"), { fileAccess: true });
    }
    params.data = await readLocalFile(url);
  } else {
    params.url = url;
    params.withCredentials = true;
  }
  const task = pdfjsLib.getDocument(params);
  task.onPassword = (update, reason) => {
    const password = prompt(reason === pdfjsLib.PasswordResponses.INCORRECT_PASSWORD
      ? "Incorrect password. Enter the PDF password:"
      : "This PDF is password protected. Enter the password:");
    if (password === null) task.destroy();
    else update(password);
  };
  return task.promise;
}

async function load() {
  if (!sourceUrl) {
    showStatus("No PDF to open.", "Open a PDF link or file to use this viewer.");
    return;
  }
  fileName = decodeURIComponent(sourceUrl.split(/[?#]/)[0].split("/").pop() || "") || "document.pdf";
  document.title = fileName;
  elements.title.textContent = fileName;
  try {
    pdfDocument = await openDocument(sourceUrl);
  } catch (error) {
    if (error.fileAccess) {
      showStatus("Chrome has not allowed this extension to read local files.",
        "Turn on “Allow access to file URLs” for PDF Better Copy, then reload this tab.",
        { label: "Open extension settings", run: () => chrome.tabs.create({ url: `chrome://extensions/?id=${chrome.runtime.id}` }) });
    } else {
      showStatus("This PDF could not be opened.", error?.message || String(error),
        { label: "Open in Chrome’s viewer", run: openInChrome });
    }
    return;
  }
  elements.status.hidden = true;
  pdfViewer.setDocument(pdfDocument);
  linkService.setDocument(pdfDocument, null);
  elements["page-count"].textContent = String(pdfDocument.numPages);
  elements["page-number"].disabled = false;
  elements.zoom.disabled = false;
  pdfDocument.getMetadata().then(({ info }) => {
    const title = typeof info?.Title === "string" ? info.Title.trim() : "";
    if (title) {
      document.title = title;
      elements.title.textContent = title;
      elements.title.title = fileName;
    }
  }).catch(() => {});
}

// ---------------------------------------------------------------------------
// Toolbar

function setZoom(value) {
  fitMode = value;
  pdfViewer.currentScaleValue = value;
}

function stepZoom(steps, origin) {
  pdfViewer.updateScale({ steps, origin });
  fitMode = String(pdfViewer.currentScale);
}

elements["zoom-in"].addEventListener("click", () => stepZoom(ZOOM_STEPS));
elements["zoom-out"].addEventListener("click", () => stepZoom(-ZOOM_STEPS));
elements.zoom.addEventListener("change", () => {
  const percent = parseFloat(elements.zoom.value);
  if (percent > 0) setZoom(String(Math.min(Math.max(percent, 10), 1000) / 100));
  else elements.zoom.value = `${Math.round(pdfViewer.currentScale * 100)}%`;
});
elements.fit.addEventListener("click", () => {
  const next = fitMode === "page-fit" ? "page-width" : "page-fit";
  setZoom(next);
  elements.fit.title = next === "page-fit" ? "Fit to width" : "Fit to page";
  elements.fit.setAttribute("aria-label", elements.fit.title);
});
elements.rotate.addEventListener("click", () => {
  pdfViewer.pagesRotation = (pdfViewer.pagesRotation + 90) % 360;
});
elements["page-number"].addEventListener("change", () => {
  const page = parseInt(elements["page-number"].value, 10);
  if (pdfDocument && page >= 1 && page <= pdfDocument.numPages) pdfViewer.currentPageNumber = page;
  else elements["page-number"].value = String(pdfViewer.currentPageNumber);
});
for (const input of [elements["page-number"], elements.zoom]) {
  input.addEventListener("focus", () => input.select());
}

async function documentBlob() {
  return new Blob([await pdfDocument.getData()], { type: "application/pdf" });
}

async function download() {
  if (!pdfDocument) return;
  const url = URL.createObjectURL(await documentBlob());
  const link = document.createElement("a");
  link.href = url;
  link.download = fileName.toLowerCase().endsWith(".pdf") ? fileName : `${fileName}.pdf`;
  link.click();
  setTimeout(() => URL.revokeObjectURL(url), 30000);
}

// Chrome's own PDF viewer prints the original file in a hidden frame.
async function print() {
  if (!pdfDocument) return;
  const url = URL.createObjectURL(await documentBlob());
  const frame = document.createElement("iframe");
  frame.style.cssText = "position:fixed;right:0;bottom:0;width:0;height:0;border:0;";
  frame.src = url;
  frame.addEventListener("load", () => {
    setTimeout(() => {
      try {
        frame.contentWindow.focus();
        frame.contentWindow.print();
      } catch {
        window.print();
      }
    }, 300);
  }, { once: true });
  document.body.append(frame);
  setTimeout(() => {
    frame.remove();
    URL.revokeObjectURL(url);
  }, 60000);
}

function openInChrome() {
  if (sourceUrl) chrome.runtime.sendMessage({ type: "open-in-chrome", url: sourceUrl });
}

elements.download.addEventListener("click", download);
elements.print.addEventListener("click", print);
elements["open-in-chrome"].addEventListener("click", openInChrome);

// ---------------------------------------------------------------------------
// Find

function dispatchFind(type, findPrevious = false) {
  eventBus.dispatch("find", {
    source: window,
    type,
    query: elements["find-input"].value,
    caseSensitive: false,
    entireWord: false,
    highlightAll: true,
    findPrevious,
    matchDiacritics: false
  });
}

function openFind() {
  elements.findbar.hidden = false;
  elements["find-input"].focus();
  elements["find-input"].select();
}

function closeFind() {
  elements.findbar.hidden = true;
  eventBus.dispatch("findbarclose", { source: window });
  elements.viewerContainer.focus();
}

elements["find-toggle"].addEventListener("click", () => (elements.findbar.hidden ? openFind() : closeFind()));
elements["find-close"].addEventListener("click", closeFind);
elements["find-next"].addEventListener("click", () => dispatchFind("again", false));
elements["find-previous"].addEventListener("click", () => dispatchFind("again", true));
elements["find-input"].addEventListener("input", () => dispatchFind(""));
elements["find-input"].addEventListener("keydown", (event) => {
  if (event.key === "Enter") {
    event.preventDefault();
    dispatchFind("again", event.shiftKey);
  } else if (event.key === "Escape") {
    event.preventDefault();
    closeFind();
  }
});
eventBus.on("updatefindmatchescount", ({ matchesCount }) => {
  const { current, total } = matchesCount || {};
  elements["find-count"].textContent = total ? `${current}/${total}` : elements["find-input"].value ? "0/0" : "";
});
eventBus.on("updatefindcontrolstate", ({ matchesCount }) => {
  const { current, total } = matchesCount || {};
  elements["find-count"].textContent = total ? `${current}/${total}` : elements["find-input"].value ? "0/0" : "";
});

// ---------------------------------------------------------------------------
// Keyboard and zoom gestures

window.addEventListener("keydown", (event) => {
  const command = event.ctrlKey || event.metaKey;
  if (!command || event.altKey) return;
  const key = event.key.toLowerCase();
  if (key === "f") {
    event.preventDefault();
    openFind();
  } else if (key === "s") {
    event.preventDefault();
    download();
  } else if (key === "p") {
    event.preventDefault();
    print();
  } else if (key === "=" || key === "+") {
    event.preventDefault();
    stepZoom(ZOOM_STEPS);
  } else if (key === "-") {
    event.preventDefault();
    stepZoom(-ZOOM_STEPS);
  } else if (key === "0") {
    event.preventDefault();
    setZoom("page-width");
  }
});

// Fit modes follow the window size, as in Chrome's viewer.
window.addEventListener("resize", () => {
  if (pdfDocument && Number.isNaN(Number(fitMode))) pdfViewer.currentScaleValue = fitMode;
});

elements.viewerContainer.addEventListener("wheel", (event) => {
  if (!event.ctrlKey) return;
  event.preventDefault();
  stepZoom(event.deltaY < 0 ? 1 : -1, [event.clientX, event.clientY]);
}, { passive: false });

// ---------------------------------------------------------------------------
// Copy

function pageModel(pageNumber) {
  if (!pageModels.has(pageNumber)) {
    const promise = pdfDocument.getPage(pageNumber)
      .then((page) => readPageData(page, pdfjsLib.OPS))
      .then((data) => {
        const model = analyze(data);
        promise.model = model;
        return model;
      });
    promise.catch(() => pageModels.delete(pageNumber));
    pageModels.set(pageNumber, promise);
  }
  return pageModels.get(pageNumber);
}

function activeFormat() {
  return settings.enabled ? settings.format : null;
}

function plainText(selection) {
  return pdfjsLib.normalizeUnicode(selection.toString()).replace(/\0/g, "");
}

function convert(pages, format) {
  const requests = [];
  for (const page of pages) {
    const model = pageModels.get(page.pageNumber).model;
    const selection = itemSelection(page, model.items);
    if (!selection) throw new Error(`Text layer of page ${page.pageNumber} does not match its content.`);
    requests.push({ model, selection });
  }
  return serializeSelection(requests, format);
}

function showToast(message) {
  elements.toast.textContent = message;
  elements.toast.toggleAttribute("data-visible", true);
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => elements.toast.removeAttribute("data-visible"), 1600);
}

function selectionInViewer(selection) {
  return selection && !selection.isCollapsed &&
    elements.viewer.contains(selection.anchorNode) && elements.viewer.contains(selection.focusNode);
}

async function copyAsync(selection, format) {
  const pages = selectedLeaves(selection, elements.viewer);
  if (format === "plain" || !pages.length) {
    await navigator.clipboard.writeText(plainText(selection));
    showToast("Copied as plain text");
    return;
  }
  await Promise.all(pages.map((page) => pageModel(page.pageNumber)));
  let text;
  try {
    text = convert(pages, format);
  } catch (error) {
    console.error(error);
    await navigator.clipboard.writeText(plainText(selection));
    showToast("Copied as plain text: this selection could not be converted");
    return;
  }
  await navigator.clipboard.writeText(text || plainText(selection));
  showToast(`Copied as ${FORMAT_NAMES[format]}`);
}

// Runs before the PDF.js text layer handler, which would copy plain text.
document.addEventListener("copy", (event) => {
  const format = activeFormat();
  const selection = document.getSelection();
  if (!format || !selectionInViewer(selection)) return;
  const pages = selectedLeaves(selection, elements.viewer);
  if (!pages.length) return;
  event.preventDefault();
  event.stopImmediatePropagation();
  if (pages.every((page) => pageModels.get(page.pageNumber)?.model)) {
    try {
      event.clipboardData.setData("text/plain", convert(pages, format) || plainText(selection));
      showToast(`Copied as ${FORMAT_NAMES[format]}`);
    } catch (error) {
      console.error(error);
      event.clipboardData.setData("text/plain", plainText(selection));
      showToast("Copied as plain text: this selection could not be converted");
    }
    return;
  }
  // Pages that were not analyzed yet are converted after the event.
  event.clipboardData.setData("text/plain", plainText(selection));
  copyAsync(selection, format).catch((error) => {
    console.error(error);
    showToast("Copied as plain text");
  });
}, true);

const currentTab = chrome.tabs.getCurrent();
chrome.runtime.onMessage.addListener((message) => {
  if (message?.type !== "copy-as") return;
  currentTab.then((tab) => {
    if (tab?.id !== message.tabId) return;
    const selection = document.getSelection();
    if (!selectionInViewer(selection)) return;
    copyAsync(selection, message.format).catch((error) => {
      console.error(error);
      showToast("Could not copy the selection");
    });
  });
});

// ---------------------------------------------------------------------------
// Settings

function applySettings(items) {
  if ("enabled" in items) settings.enabled = items.enabled !== false;
  if ("format" in items) settings.format = items.format === "markdown" ? "markdown" : "latex";
}

chrome.storage.local.get({ enabled: true, format: "latex" }, applySettings);
chrome.storage.onChanged.addListener((changes, area) => {
  if (area !== "local") return;
  applySettings(Object.fromEntries(Object.entries(changes).map(([key, change]) => [key, change.newValue])));
});

load();
