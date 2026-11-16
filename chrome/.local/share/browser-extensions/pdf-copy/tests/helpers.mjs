// Loads fixture PDFs with the vendored PDF.js and selects text the way the viewer does.
import fs from "node:fs";
import { fileURLToPath } from "node:url";
import { analyze, readPageData, serializeSelection } from "../lib/convert.js";

// The vendored browser build expects APIs that Chromium has and Node.js 24 lacks.
Uint8Array.prototype.toHex ??= function toHex() {
  return Array.from(this, (byte) => byte.toString(16).padStart(2, "0")).join("");
};
Uint8Array.prototype.toBase64 ??= function toBase64() {
  return Buffer.from(this).toString("base64");
};
Uint8Array.fromBase64 ??= (text) => new Uint8Array(Buffer.from(text, "base64"));
Uint8Array.fromHex ??= (text) => new Uint8Array(Buffer.from(text, "hex"));
for (const Collection of [Map, WeakMap]) {
  Collection.prototype.getOrInsert ??= function getOrInsert(key, value) {
    if (!this.has(key)) this.set(key, value);
    return this.get(key);
  };
  Collection.prototype.getOrInsertComputed ??= function getOrInsertComputed(key, compute) {
    if (!this.has(key)) this.set(key, compute(key));
    return this.get(key);
  };
}

const pdfjs = await import("../vendor/pdfjs/build/pdf.min.mjs");
pdfjs.GlobalWorkerOptions.workerSrc = fileURLToPath(new URL("../vendor/pdfjs/build/pdf.worker.min.mjs", import.meta.url));

const tasks = new Map();

export function fixturePath(name) {
  return fileURLToPath(new URL(`fixtures/${name}`, import.meta.url));
}

export async function loadPage(name, pageNumber) {
  if (!tasks.has(name)) {
    tasks.set(name, pdfjs.getDocument({
      data: new Uint8Array(fs.readFileSync(fixturePath(name))),
      fontExtraProperties: true,
      isEvalSupported: false,
      verbosity: 0
    }));
  }
  const document = await tasks.get(name).promise;
  const page = await document.getPage(pageNumber);
  return analyze(await readPageData(page, pdfjs.OPS));
}

export async function pageCount(name) {
  await loadPage(name, 1);
  return (await tasks.get(name).promise).numPages;
}

// Selects the text layer characters from `from` to the end of `to`; "|" in `from` marks the start.
export function select(model, from = "|", to = null) {
  let text = "";
  const starts = model.items.map((item) => {
    const start = text.length;
    text += item.str;
    return start;
  });
  const marker = Math.max(0, from.indexOf("|"));
  const found = text.indexOf(from.replace("|", ""));
  if (found < 0) throw new Error(`"${from}" is not on the page`);
  const start = found + marker;
  const end = to === null ? text.length : text.indexOf(to, start) + to.length;
  if (to !== null && end < start + to.length) throw new Error(`"${to}" does not follow "${from}"`);
  const selection = new Map();
  model.items.forEach((item, index) => {
    const from = Math.max(start, starts[index]) - starts[index];
    const until = Math.min(end, starts[index] + item.str.length) - starts[index];
    if (until > from) selection.set(index, [[from, until]]);
  });
  return selection;
}

export function copy(model, selection, format = "latex") {
  return serializeSelection([{ model, selection }], format);
}

export async function destroyAll() {
  for (const task of tasks.values()) await task.destroy();
  tasks.clear();
}
