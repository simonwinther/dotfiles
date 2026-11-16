// Renders selected parts of analyzed PDF pages as LaTeX or Markdown.
import { parseRow } from "./layout.js";
import { buildUnits, findChains, markMath } from "./structure.js";
import {
  BIG_OPERATORS,
  BINARY_OPERATORS,
  CLOSING_DELIMITERS,
  EXTRA_OPERATOR_NAMES,
  MATH_SYMBOLS,
  OPENING_DELIMITERS,
  OPERATOR_NAMES,
  RELATIONS,
  STYLE_COMMANDS,
  TEXT_ACCENTS,
  decodeMathAlphanumeric
} from "./symbols.js";

const SECTION_COMMANDS = ["section", "subsection", "subsubsection"];
const LATEX_TEXT_REPLACEMENTS = new Map(Object.entries({
  "\\": "\\textbackslash{}", "~": "\\textasciitilde{}", "^": "\\textasciicircum{}",
  "#": "\\#", "$": "\\$", "%": "\\%", "&": "\\&", "_": "\\_", "{": "\\{", "}": "\\}",
  "–": "--", "—": "---", "“": "``", "”": "''", "‘": "`", "’": "'", "…": "\\ldots{}",
  "<": "\\textless{}", ">": "\\textgreater{}", "•": "\\textbullet{}", " ": "~"
}));

// ---------------------------------------------------------------------------
// Selection

function glyphSelected(glyph, selection) {
  if (glyph.synthetic) return glyphSelected(glyph.neighbor, selection);
  const ranges = selection.get(glyph.item);
  if (!ranges) return false;
  return ranges.some(([start, end]) => glyph.ext || (glyph.offset < end && glyph.offset + glyph.length > start));
}

function atomSelected(atom, selection) {
  if (atom.kind === "glyph") return glyphSelected(atom, selection);
  return atom.glyphs.some((glyph) => glyphSelected(glyph, selection));
}

function nodeSelected(node, selection) {
  if (node.space) return glyphSelected(node.atom, selection);
  return atomSelected(node.atom, selection) ||
    (node.sup || []).some((child) => nodeSelected(child, selection)) ||
    (node.sub || []).some((child) => nodeSelected(child, selection));
}

function unitSelected(unit, selection) {
  return unit.nodes?.some((node) => nodeSelected(node, selection));
}

// ---------------------------------------------------------------------------
// Math

function group(latex) {
  return /^[A-Za-z0-9]$/.test(latex) ? latex : `{${latex}}`;
}

function styleCommand(style, body) {
  if (style === "bfcal") return `\\boldsymbol{\\mathcal{${body}}}`;
  return `${STYLE_COMMANDS[style]}{${body}}`;
}

export function glyphLatex(glyph) {
  if (glyph.ext) {
    if (glyph.ext.kind === "radical") return "\\surd";
    return glyph.ext.latex || "";
  }
  if (glyph.piece) return glyph.piece[0];
  let ch = glyph.ch;
  const { font } = glyph;
  let style = null;
  const decoded = decodeMathAlphanumeric(ch);
  if (decoded) {
    ch = decoded.char;
    style = decoded.style === "it" ? null : decoded.style;
  } else if (/^[A-Za-z]$/.test(ch)) {
    if (font.style === "calLetters") style = /^[A-Z]$/.test(ch) ? "cal" : null;
    else if (font.style && font.style !== "bfsym") style = font.style;
    else if (font.style === "bfsym") style = "bfcal";
    else if (!font.math) style = font.bold ? "bf" : font.italic ? null : font.mono ? "tt" : font.sans && !font.sansBody ? "sf" : "rm";
    else if (font.math === "unicode") style = "rm";
  } else if (/^[0-9]$/.test(ch)) {
    if (!font.math && font.bold) style = "bf";
  } else if (font.style === "bfit" || (!font.math && font.bold)) {
    style = /^[Ͱ-Ͽ]$/u.test(ch) ? "bfit" : null;
  }
  if (ch === "̸") return "\\not";
  if (ch === "7" && font.math === "symbol") return "\\mapsto";
  let latex = MATH_SYMBOLS.get(ch) ?? ch;
  if (/^[\x00-\x1f-]$/u.test(latex)) return "";
  if (!style) return latex;
  if (style === "bfit" && !/^[A-Za-z]$/.test(ch)) return `\\boldsymbol{${latex}}`;
  return styleCommand(style, latex);
}

function delimiterLatex(side, latex) {
  return `\\${side}${latex === "." ? "." : latex}`;
}

function renderAtom(atom, options) {
  switch (atom.kind) {
    case "glyph":
      return glyphLatex(atom);
    case "symbol":
      return atom.latex;
    case "accent":
      return `${atom.accent}{${renderRow(atom.body, options)}}`;
    case "frac":
      return `\\frac{${renderRow(atom.numerator, options)}}{${renderRow(atom.denominator, options)}}`;
    case "sqrt": {
      const body = renderRow(atom.body, options);
      return atom.index ? `\\sqrt[${renderRow(atom.index, options)}]{${body}}` : `\\sqrt{${body}}`;
    }
    case "overline":
    case "underline":
      return `\\${atom.kind}{${renderRow(atom.body, options)}}`;
    case "bigop": {
      let latex = atom.latex;
      if (atom.lower.length) latex += `_${group(renderRow(atom.lower, { ...options, compact: true }))}`;
      if (atom.upper.length) latex += `^${group(renderRow(atom.upper, { ...options, compact: true }))}`;
      return latex;
    }
    case "leftright":
      return `${delimiterLatex("left", atom.open)} ${renderRow(atom.body, options)} ${delimiterLatex("right", atom.close)}`;
    case "matrix":
      return renderMatrix(atom, options);
    default:
      return "";
  }
}

function renderMatrix(atom, options) {
  const rows = atom.cells.map((row) => row.map((cell) => (cell.length ? renderRow(cell, options) : "")).join(" & "));
  const separator = options.display ? " \\\\\n" : " \\\\ ";
  const open = options.display ? `\\begin{${atom.environment}}\n` : `\\begin{${atom.environment}} `;
  const close = options.display ? `\n\\end{${atom.environment}}` : ` \\end{${atom.environment}}`;
  let latex = open + rows.join(separator) + close;
  if (atom.open || atom.close) latex = `${delimiterLatex("left", atom.open || ".")} ${latex} ${delimiterLatex("right", atom.close || ".")}`;
  return latex;
}

function renderRow(atoms, options) {
  return renderNodes(parseRow(atoms), { ...options, display: false });
}

export function renderNodes(nodes, options) {
  const units = buildUnits(nodes);
  for (const unit of units) unit.math = unit.type !== "space";
  return renderMathUnits(units, options);
}

function scriptLatex(nodes, options) {
  const visible = nodes.filter((node) => !node.space);
  if (visible.length && visible.every((node) => node.atom.kind === "glyph" && /^[′″‴']$/.test(node.atom.ch) && !node.sup && !node.sub)) {
    return { primes: visible.map((node) => ({ "′": "'", "″": "''", "‴": "'''", "'": "'" })[node.atom.ch]).join("") };
  }
  return { latex: renderNodes(nodes, options) };
}

function renderNode(node, options, baseOverride) {
  let latex = baseOverride ?? renderAtom(node.atom, options);
  const scriptOptions = { ...options, compact: true, display: false };
  const sup = node.sup ? scriptLatex(node.sup, scriptOptions) : null;
  if (sup?.primes) latex += sup.primes;
  if (node.sub) latex += `_${group(renderNodes(node.sub, scriptOptions))}`;
  if (sup && !sup.primes) latex += `^${group(sup.latex)}`;
  return latex;
}

// Vertical bars are relations when TeX spaced them as such, otherwise delimiters.
function barLatex(node, units, index) {
  const { atom } = node;
  if (atom.kind !== "glyph" || !["∥", "|", "∣"].includes(atom.ch)) return null;
  const unit = units[index];
  const before = units.slice(0, index).reverse().find((other) => other.type !== "space" && other.type !== "break");
  const after = units.slice(index + 1).find((other) => other.type !== "space" && other.type !== "break");
  const spaced = before && after && unit.x0 - before.x1 >= 0.2 * unit.size && after.x0 - unit.x1 >= 0.2 * unit.size;
  if (atom.ch === "∥") return spaced ? "\\parallel" : "\\|";
  return spaced ? "\\mid" : "|";
}

function tokenClass(latex, atom) {
  const delimiter = latex.match(/^\\(left|right)(?![A-Za-z])/);
  const core = delimiter ? latex.slice(delimiter[0].length) : latex;
  if (delimiter?.[1] === "left") return "open";
  if (delimiter?.[1] === "right") return "close";
  if (RELATIONS.has(latex) || latex === ":" || latex.startsWith("\\not")) return "rel";
  if (BINARY_OPERATORS.has(latex)) return "bin";
  if (latex === "," || latex === ";") return "punct";
  if (OPENING_DELIMITERS.has(core)) return "open";
  if (CLOSING_DELIMITERS.has(core)) return "close";
  if (BIG_OPERATORS.has(latex) || atom?.kind === "bigop") return "op";
  return "ord";
}

function wordTokens(unit, options) {
  const text = unit.text;
  const last = unit.nodes.at(-1);
  const styled = unit.nodes.every((node) => node.atom.kind === "glyph" &&
    (node.atom.font?.bold || node.atom.font?.italic || node.atom.font?.mono || (node.atom.font?.sans && !node.atom.font.sansBody)));
  if (styled || unit.nodes.some((node) => node.atom.kind !== "glyph" || node.atom.font?.math)) {
    return unit.nodes.map((node) => ({ latex: renderNode(node, options), cls: "ord" }));
  }
  let base;
  let cls = "ord";
  if (OPERATOR_NAMES.has(text)) {
    base = `\\${text}`;
    cls = "op";
  } else if (EXTRA_OPERATOR_NAMES.has(text)) {
    base = `\\operatorname{${text}}`;
    cls = "op";
  } else if (unit.beforeParen && text.length > 1) {
    base = `\\operatorname{${text}}`;
    cls = "op";
  } else if (text.length === 1) {
    base = `\\mathrm{${text}}`;
  } else if (text.length <= 3 && !unit.spaced) {
    base = `\\mathrm{${text}}`;
  } else {
    base = `\\text{${unit.spaceBefore ? " " : ""}${text}${unit.spaceAfter ? " " : ""}}`;
    cls = "text";
  }
  return [{ latex: renderNode(last, options, base), cls }];
}

export function renderMathUnits(units, options) {
  const tokens = [];
  units.forEach((unit, index) => {
    if (unit.type === "space" || unit.type === "break") {
      const previous = units.slice(0, index).reverse().find((other) => other.type !== "space" && other.type !== "break");
      if (tokens.length && (unit.type === "break" || unit.x1 - unit.x0 >= 0.2 * (previous?.size || unit.size))) {
        tokens.at(-1).spaceAfter = true;
      }
      return;
    }
    if (unit.soft) return;
    if (unit.type === "word") {
      unit.spaceBefore = units[index - 1]?.type === "space" && index > 0;
      unit.spaceAfter = units[index + 1]?.type === "space";
      unit.spaced = unit.spaceBefore || unit.spaceAfter;
      unit.beforeParen = units[index + 1]?.text === "(" && units[index + 1].x0 - unit.x1 < 0.12 * unit.size;
      tokens.push(...wordTokens(unit, options));
      return;
    }
    for (const node of unit.nodes) {
      const base = barLatex(node, units, index) ?? renderAtom(node.atom, options);
      const latex = renderNode(node, options, base);
      if (!latex) continue;
      tokens.push({ latex, cls: tokenClass(base, node.atom) });
    }
  });
  return joinTokens(tokens, options);
}

function joinTokens(tokens, options = {}) {
  const { compact } = options;
  let out = "";
  tokens.forEach((token, index) => {
    const previous = tokens[index - 1];
    if (token.cls === "rel") {
      out = compact ? out.trimEnd() + token.latex : out.trimEnd() + (out ? " " : "") + token.latex + " ";
    } else if (token.cls === "bin") {
      const unary = !previous || ["rel", "bin", "open", "punct", "op"].includes(previous.cls);
      out = unary || compact ? out + token.latex : `${out.trimEnd()} ${token.latex} `;
    } else if (token.cls === "punct") {
      out = out.trimEnd() + token.latex + (compact ? "" : " ");
    } else {
      const afterCommand = (/\\[A-Za-z]+$/.test(out) || /[_^][A-Za-z0-9]$/.test(out)) && /^[A-Za-z]/.test(token.latex);
      const spaced = !compact && previous && !/\s$/.test(out) && previous.cls !== "open" && token.cls !== "close" &&
        !(previous.cls === "op" && token.cls === "open") &&
        (previous.cls === "op" || previous.cls === "text" || token.cls === "text" || (token.cls === "op" && previous.cls !== "open") || previous.spaceAfter);
      if (afterCommand || spaced) out += " ";
      out += token.latex;
    }
  });
  return out.trim();
}

// ---------------------------------------------------------------------------
// Text

function escapeLatex(text) {
  return Array.from(text, (char) => LATEX_TEXT_REPLACEMENTS.get(char) ?? char).join("");
}

function textStyle(atom) {
  const font = atom.kind === "glyph" ? atom.font : atom.font || atom.glyphs?.[0]?.font;
  if (!font) return "";
  return `${font.bold ? "b" : ""}${font.italic ? "i" : ""}${font.mono ? "m" : ""}`;
}

function nodeText(node) {
  const { atom } = node;
  if (atom.kind === "accent") {
    return (atom.body[0].ch + (TEXT_ACCENTS.get(atom.accentChar) || "")).normalize("NFC");
  }
  if (atom.kind !== "glyph") return "";
  if (/^[\x00-\x1f-]$/u.test(atom.ch)) return "";
  return atom.ch;
}

function scriptText(nodes) {
  return nodes.filter((node) => !node.space).map(nodeText).join("");
}

// PDF.js reports small kerning gaps as spaces; TeX never shrinks a word space this far.
function isKerningSpace(unit, next) {
  if (unit.type !== "space") return false;
  const width = unit.x1 - unit.x0;
  if (width < 0.19 * unit.size) return true;
  return next?.type === "punct" && /^[.,;:!?)\]]$/.test(next.text) && width < 0.3 * unit.size;
}

// Emits a flat token list for one run of units: text pieces, spaces, and inline math.
function paragraphTokens(units, chains, selection, format) {
  const tokens = [];
  const chainAt = new Map(chains.map((chain) => [chain.start, chain]));
  for (let index = 0; index < units.length; index++) {
    const unit = units[index];
    const chain = chainAt.get(index);
    if (chain) {
      const slice = units.slice(chain.start, chain.end + 1);
      if (slice.some((part) => unitSelected(part, selection))) {
        tokens.push({ kind: "math", latex: renderMathUnits(slice, { display: false }) });
      }
      index = chain.end;
      continue;
    }
    if (unit.type === "space" || unit.type === "break") {
      if (!unit.join && !isKerningSpace(unit, units[index + 1])) tokens.push({ kind: "space" });
      continue;
    }
    if (unit.soft) continue;
    for (const node of unit.nodes) {
      if (!nodeSelected(node, selection)) continue;
      let text = nodeText(node);
      const style = textStyle(node.atom);
      const scripts = [];
      if (node.sup) scripts.push(["sup", scriptText(node.sup)]);
      if (node.sub) scripts.push(["sub", scriptText(node.sub)]);
      // A literal dollar sign would start inline math in Markdown.
      text = format === "latex" ? escapeLatex(text) : text.replace(/\$/g, "\\$");
      tokens.push({ kind: "text", text, style });
      for (const [kind, value] of scripts) {
        if (!value) continue;
        tokens.push({
          kind: "raw",
          text: format === "latex"
            ? `\\text${kind === "sup" ? "superscript" : "subscript"}{${escapeLatex(value)}}`
            : `<${kind}>${value}</${kind}>`
        });
      }
    }
  }
  return tokens;
}

function wrapStyle(text, style, format) {
  if (!style || !text) return text;
  if (format === "latex") {
    let result = text;
    if (style.includes("m")) result = `\\texttt{${result}}`;
    if (style.includes("i")) result = `\\emph{${result}}`;
    if (style.includes("b")) result = `\\textbf{${result}}`;
    return result;
  }
  if (style.includes("m")) return `\`${text}\``;
  const marker = (style.includes("b") ? "**" : "") + (style.includes("i") ? "*" : "");
  return marker ? `${marker}${text}${[...marker].reverse().join("")}` : text;
}

function joinTokensToText(tokens, format, { plainStyle = false } = {}) {
  // Merge text tokens with the same style across single spaces.
  const runs = [];
  for (const token of tokens) {
    const last = runs.at(-1);
    if (token.kind === "space") {
      if (last && last.kind !== "space") runs.push({ kind: "space" });
      continue;
    }
    if (token.kind === "text") {
      const style = plainStyle ? "" : token.style;
      if (last?.kind === "text" && last.style === style) {
        last.text += token.text;
        continue;
      }
      const beforeSpace = runs.at(-2);
      if (last?.kind === "space" && beforeSpace?.kind === "text" && beforeSpace.style === style && style) {
        runs.pop();
        beforeSpace.text += " " + token.text;
        continue;
      }
      runs.push({ kind: "text", text: token.text, style });
      continue;
    }
    runs.push(token);
  }
  let out = "";
  for (const run of runs) {
    if (run.kind === "space") out += " ";
    else if (run.kind === "text") out += wrapStyle(run.text, run.style, format);
    else if (run.kind === "math") out += format === "latex" ? `\\(${run.latex}\\)` : `$${run.latex}$`;
    else out += run.text;
  }
  return out.replace(/ {2,}/g, " ").trim();
}

// ---------------------------------------------------------------------------
// Blocks

function linesToUnits(lines, skipUnits = 0) {
  const units = [];
  lines.forEach((line, lineIndex) => {
    let lineUnits = buildUnits(line.nodes);
    if (lineIndex === 0 && skipUnits) lineUnits = lineUnits.slice(skipUnits);
    while (lineUnits[0]?.type === "space") lineUnits.shift();
    if (units.length) {
      const breakUnit = { type: "break", nodes: [], text: " ", x0: 0, x1: 0, size: line.size };
      const lastUnit = units.at(-1);
      const beforeLast = units.at(-2);
      const firstUnit = lineUnits[0];
      if (lastUnit?.type === "punct" && lastUnit.text === "-" && beforeLast?.type === "word" &&
        firstUnit?.type === "word" && /^\p{Ll}/u.test(firstUnit.text)) {
        lastUnit.soft = true;
        breakUnit.join = true;
      }
      units.push(breakUnit);
    }
    units.push(...lineUnits);
  });
  return units;
}

function paragraphText(lines, selection, format, skipUnits = 0, options = {}) {
  const units = markMath(linesToUnits(lines, skipUnits));
  const chains = findChains(units);
  return joinTokensToText(paragraphTokens(units, chains, selection, format), format, options);
}

function linesSelected(lines, selection) {
  return lines.some((line) => line.atoms.some((atom) => atomSelected(atom, selection)));
}

function displayText(block, selection, format) {
  const rendered = block.lines.map((line) => {
    let nodes = line.nodes;
    if (line.equationNumber) {
      const units = buildUnits(nodes);
      const visible = units.filter((unit) => unit.type !== "space");
      const drop = new Set(visible.slice(-line.equationNumber).flatMap((unit) => unit.nodes));
      nodes = nodes.filter((node) => !drop.has(node));
    }
    const units = buildUnits(nodes);
    for (const unit of units) unit.math = unit.type !== "space";
    const display = units.some((unit) => unit.nodes.some((node) => node.atom.kind === "matrix"));
    return { latex: renderMathUnits(units, { display }), relationX: firstRelationX(units) };
  });
  const aligned = rendered.length > 1 && rendered.every((line) => line.relationX !== null) &&
    rendered.every((line) => Math.abs(line.relationX - rendered[0].relationX) < 2);
  if (aligned) {
    const body = rendered.map((line) => alignAtRelation(line.latex)).join(" \\\\\n");
    return format === "latex"
      ? `\\begin{align*}\n${body}\n\\end{align*}`
      : `$$\n\\begin{aligned}\n${body}\n\\end{aligned}\n$$`;
  }
  return rendered.map((line) => (format === "latex" ? `\\[\n${line.latex}\n\\]` : `$$\n${line.latex}\n$$`)).join("\n\n");
}

function firstRelationX(units) {
  for (const unit of units) {
    for (const node of unit.nodes) {
      const latex = renderAtom(node.atom, {});
      if (tokenClass(latex, node.atom) === "rel") return node.atom.x0;
    }
  }
  return null;
}

function alignAtRelation(latex) {
  const match = latex.match(/(^|\s)(=|<|>|\\(?:leq|geq|neq|approx|equiv|sim|le|ge|to|Rightarrow|Leftrightarrow|in|subseteq|cong|propto)\b)/);
  if (!match) return latex;
  const position = match.index + match[1].length;
  return `${latex.slice(0, position)}&${latex.slice(position)}`;
}

function stripHeadingNumber(text) {
  return text.replace(/^\s*\d+(?:\.\d+)*[.)]?\s+/, "");
}

function renderBlocks(blocks, selection, format) {
  const parts = [];
  for (const block of blocks) {
    if (block.type === "list") {
      const text = renderList(block, selection, format);
      if (text) parts.push(text);
      continue;
    }
    if (!linesSelected(block.lines, selection)) continue;
    if (block.type === "furniture") {
      parts.push({ furniture: true, text: paragraphText(block.lines, selection, format) });
    } else if (block.type === "display") {
      parts.push(displayText(block, selection, format));
    } else if (block.type === "heading") {
      const text = paragraphText(block.lines, selection, format, 0, { plainStyle: true });
      if (!text) continue;
      if (format === "latex") parts.push(`\\${SECTION_COMMANDS[block.level - 1] || "paragraph"}{${stripHeadingNumber(text)}}`);
      else parts.push(`${"#".repeat(block.level)} ${text}`);
    } else {
      const text = paragraphText(block.lines, selection, format);
      if (text) parts.push(text);
    }
  }
  return parts;
}

function labelSelected(paragraph, selection) {
  const units = buildUnits(paragraph.lines[0].nodes).slice(0, paragraph.label.consumed);
  return units.some((unit) => unitSelected(unit, selection));
}

function renderList(list, selection, format) {
  const rendered = [];
  let firstNumber = null;
  for (const item of list.items) {
    const [first, ...rest] = item.blocks;
    const asItem = labelSelected(first, selection);
    const head = linesSelected(first.lines, selection)
      ? paragraphText(first.lines, selection, format, asItem ? first.label.consumed + 1 : 0)
      : "";
    const body = renderBlocks(rest, selection, format).map((part) => part.text ?? part);
    if (!head && !body.length) continue;
    if (!asItem) {
      rendered.push({ plain: [head, ...body].filter(Boolean) });
      continue;
    }
    if (firstNumber === null) firstNumber = parseInt(item.label, 10);
    rendered.push({ label: item.label, content: [head, ...body].filter(Boolean) });
  }
  if (!rendered.length) return "";
  const parts = [];
  const items = rendered.filter((entry) => entry.label);
  const plain = rendered.filter((entry) => entry.plain);
  for (const entry of plain) parts.push(...entry.plain);
  if (!items.length) return parts.join("\n\n");
  const numeric = items.every((entry) => /^\d+[.)]$/.test(entry.label));
  const bullet = items.every((entry) => /^[•◦▪‣∙·–—*-]$/.test(entry.label));
  if (format === "latex") {
    const environment = bullet ? "itemize" : "enumerate";
    const counter = numeric && firstNumber && firstNumber !== 1 ? `\\setcounter{enumi}{${firstNumber - 1}}\n` : "";
    const lines = items.map((entry) => {
      const label = numeric || bullet ? "" : `[${escapeLatex(entry.label)}]`;
      return `\\item${label} ${entry.content.join("\n\n")}`;
    });
    parts.push(`\\begin{${environment}}\n${counter}${lines.join("\n")}\n\\end{${environment}}`);
  } else {
    for (const entry of items) {
      const marker = numeric ? `${parseInt(entry.label, 10)}. ` : bullet ? "- " : `${entry.label} `;
      const indent = " ".repeat(marker.length);
      const [head, ...rest] = entry.content;
      const lines = [marker + head, ...rest.map((part) => part.split("\n").map((line) => (line ? indent + line : "")).join("\n"))];
      parts.push(lines.join("\n\n"));
    }
  }
  return parts.join("\n\n");
}

// pages: [{ model, selection: Map<itemIndex, [start, end][]> }]
export function serializeSelection(pages, format) {
  const outputs = [];
  for (const { model, selection } of pages) {
    const parts = renderBlocks(model.blocks, selection, format);
    const content = parts.filter((part) => typeof part === "string");
    const furniture = parts.filter((part) => typeof part !== "string").map((part) => part.text);
    outputs.push(...(content.length || pages.length > 1 ? content : furniture));
  }
  let result = "";
  for (const part of outputs) {
    if (!part) continue;
    if (!result) result = part;
    else if (/[\p{L},]$/u.test(result) && /^\p{Ll}/u.test(part) && !part.includes("\n")) result += ` ${part}`;
    else result += `\n\n${part}`;
  }
  return result;
}
