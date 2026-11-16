// Separates inline math from prose and groups lines into document blocks.
import { BINARY_OPERATORS, EXTRA_OPERATOR_NAMES, MATH_SYMBOLS, OPERATOR_NAMES, RELATIONS, TEXT_ACCENTS, decodeMathAlphanumeric } from "./symbols.js";

const SENTENCE_PUNCTUATION = new Set([".", ",", ";", ":", "!", "?"]);
const TEXT_SYMBOLS = new Set(["…", "–", "—", "‘", "’", "“", "”", "•", "·", "°", "©", "®", "™", "§", "¶", "†", "‡", "′", "″", "«", "»", "„", "‚"]);
const RELATION_CHARS = new Set(["=", "<", ">", "≤", "≥", "≠", "≈", "≡", "→", "⇒", "⇔", "∈", "⊂", "⊆", ":"]);
const BINARY_CHARS = new Set(["+", "−", "×", "÷", "±", "∓", "*", "/", "·", "-"]);
const OPENERS = new Set(["(", "[", "{"]);
const CLOSERS = new Set([")", "]", "}"]);
const LABEL_RE = /^(\d{1,3}[.)]|\(?[a-zA-Z]\)|\([ivxlc]{1,5}\)|[ivxlc]{1,5}[.)]|[•◦▪‣∙·–—*-])$/;

// ---------------------------------------------------------------------------
// Units: words, numbers, and single atoms

function letterOf(atom) {
  if (atom.kind === "accent" && atom.body.length === 1 && atom.body[0].kind === "glyph" &&
    !atom.body[0].font.math && TEXT_ACCENTS.has(atom.accentChar) && /^\p{L}$/u.test(atom.body[0].ch)) {
    return (atom.body[0].ch + TEXT_ACCENTS.get(atom.accentChar)).normalize("NFC");
  }
  if (atom.kind !== "glyph" || atom.ext || atom.font.math) return null;
  if (!/^[A-Za-z]$/.test(atom.ch) && MATH_SYMBOLS.has(atom.ch)) return null;
  return /^\p{L}+$/u.test(atom.ch) ? atom.ch : null;
}

function nodeType(node) {
  if (node.space) return "space";
  const { atom } = node;
  const letter = letterOf(atom);
  if (letter !== null) return "letter";
  if (atom.kind !== "glyph") return atom.synthetic ? "punct" : "math";
  if (atom.ext) return "math";
  // Scripts with letters make a formula; digits after punctuation are footnote marks.
  if ((node.sup || node.sub) && (hasMathScripts(node) || scriptHasLetter(node))) return "math";
  if (atom.font.math) return "math";
  if (decodeMathAlphanumeric(atom.ch)) return "math";
  if (/^[Ͱ-Ͽ]$/u.test(atom.ch)) return "math";
  if (/^\p{Nd}$/u.test(atom.ch)) return node.sup || node.sub ? "math" : "digit";
  if (MATH_SYMBOLS.has(atom.ch) && !TEXT_SYMBOLS.has(atom.ch) && !/^[\x20-\x7e]$/.test(atom.ch)) return "math";
  return "punct";
}

function hasMathScripts(node) {
  for (const list of [node.sup, node.sub]) {
    if (!list) continue;
    for (const child of list) {
      if (child.space) continue;
      const type = nodeType(child);
      if (type === "math") return true;
    }
  }
  return false;
}

function scriptHasLetter(node) {
  return [node.sup, node.sub].some((list) => (list || []).some((child) => !child.space && letterOf(child.atom) !== null));
}

function isTight(left, right) {
  return right.x0 - left.x1 < 0.12 * Math.max(left.size, right.size);
}

function nodeExtent(node) {
  const atoms = [node.atom];
  for (const list of [node.sup, node.sub]) {
    for (const child of list || []) atoms.push(...nodeAtoms(child));
  }
  return {
    x0: Math.min(...atoms.map((atom) => atom.x0)),
    x1: Math.max(...atoms.map((atom) => atom.x1))
  };
}

function nodeAtoms(node) {
  const atoms = [node.atom];
  for (const list of [node.sup, node.sub]) {
    for (const child of list || []) atoms.push(...nodeAtoms(child));
  }
  return atoms;
}

export function buildUnits(nodes) {
  const units = [];
  for (const node of nodes) {
    const type = nodeType(node);
    const extent = node.space ? { x0: node.atom.x0, x1: node.atom.x1 } : nodeExtent(node);
    const previous = units.at(-1);
    const size = node.atom.size;
    if (previous && previous.type !== "space" && !node.space && node.atom.x0 - previous.x1 > 0.2 * Math.max(size, previous.size)) {
      // Large gaps without a space glyph still separate words.
      units.push({ type: "space", nodes: [], text: " ", x0: previous.x1, x1: node.atom.x0, size, seed: false, virtual: true });
    }
    const last = units.at(-1);
    if (last && type === "letter" && last.type === "word" && isTight(last, { x0: node.atom.x0, x1: extent.x1, size }) &&
      !node.atom.rotated && !last.nodes.at(-1).sup && !last.nodes.at(-1).sub) {
      last.nodes.push(node);
      last.text += letterOf(node.atom);
      last.x1 = extent.x1;
      if (hasMathScripts(node)) last.seed = last.scripted = true;
      continue;
    }
    if (last && (type === "digit" || (type === "punct" && node.atom.ch === ".")) && last.type === "number" &&
      isTight(last, { x0: node.atom.x0, x1: extent.x1, size }) && !last.nodes.at(-1).sup && !last.nodes.at(-1).sub) {
      last.nodes.push(node);
      last.text += node.atom.ch;
      last.x1 = extent.x1;
      continue;
    }
    units.push({
      type: type === "letter" ? "word" : type === "digit" ? "number" : type,
      nodes: [node],
      text: type === "letter" ? letterOf(node.atom) : node.atom.ch ?? "",
      x0: extent.x0,
      x1: extent.x1,
      size,
      seed: type === "math" || (type === "letter" && hasMathScripts(node)),
      scripted: type === "letter" && hasMathScripts(node)
    });
  }
  // A single letter with scripts is a variable, not a word with a footnote mark.
  for (const unit of units) {
    const last = unit.nodes?.at(-1);
    if (unit.type === "word" && last && (last.sup || last.sub) && (unit.text.length === 1 || scriptHasLetter(last))) {
      unit.seed = unit.scripted = true;
    }
  }
  // A trailing period is sentence punctuation, not part of a number.
  for (const unit of units) {
    if (unit.type === "number" && unit.text.endsWith(".") && unit.nodes.length > 1) {
      const last = unit.nodes.pop();
      unit.text = unit.text.slice(0, -1);
      units.splice(units.indexOf(unit) + 1, 0, {
        type: "punct", nodes: [last], text: ".", x0: last.atom.x0, x1: last.atom.x1, size: last.atom.size, seed: false
      });
    }
  }
  return units;
}

// ---------------------------------------------------------------------------
// Inline math chains

function neighbor(units, index, direction) {
  let crossed = false;
  for (let cursor = index + direction; cursor >= 0 && cursor < units.length; cursor += direction) {
    if (units[cursor].type === "break") crossed = true;
    else if (units[cursor].type !== "space") return crossed ? { ...units[cursor], acrossBreak: true, original: units[cursor] } : units[cursor];
  }
  return null;
}

function tightNeighbor(units, index, direction) {
  const next = units[index + direction];
  if (!next || next.type === "space" || next.type === "break") return null;
  const [left, right] = direction > 0 ? [units[index], next] : [next, units[index]];
  return isTight(left, right) ? next : null;
}

function isOperatorUnit(unit) {
  return unit.type === "punct" && (RELATION_CHARS.has(unit.text) || BINARY_CHARS.has(unit.text));
}

function isMathOperator(unit) {
  if (!unit || !unit.math) return false;
  if (isOperatorUnit(unit)) return true;
  if (unit.nodes.length !== 1) return false;
  const atom = unit.nodes[0].atom;
  const latex = atom.kind === "symbol" ? atom.latex : atom.kind === "glyph" ? MATH_SYMBOLS.get(atom.ch) ?? atom.ch : null;
  return RELATIONS.has(latex) || BINARY_OPERATORS.has(latex);
}

function near(a, b) {
  return Boolean(b) && (b.acrossBreak || Math.max(b.x0 - a.x1, a.x0 - b.x1) < 1.3 * Math.max(a.size, b.size));
}

function isWordStyle(unit, test) {
  return unit.nodes.every((node) => node.atom.kind === "glyph" && test(node.atom.font));
}

export function markMath(units) {
  for (const unit of units) unit.math = unit.type === "math";

  for (const unit of units) {
    if (unit.scripted) unit.math = true;
  }
  // A bold letter between ordinary words is a vector such as \mathbf{v}.
  units.forEach((unit, index) => {
    const special = (font) => font.bold || font.mono || font.italic || (font.sans && !font.sansBody);
    if (unit.type !== "word" || !isWordStyle(unit, special)) return;
    const style = (font) => `${font.bold}${font.sans}${font.mono}${font.italic}`;
    const own = style(unit.nodes[0].atom.font);
    const neighbors = [neighbor(units, index, -1), neighbor(units, index, 1)]
      .filter((other) => other && other.type === "word" && !other.math && !other.scripted);
    const boldNeighbors = neighbors.filter((other) => isWordStyle(other, (font) => style(font) === own));
    if (unit.text.length === 1 && boldNeighbors.length === 0 && /^[a-zA-Z]$/.test(unit.text)) {
      unit.math = true;
      unit.seed = true;
    }
    if (unit.text.length <= 3 && (tightNeighbor(units, index, 1)?.type === "math" || tightNeighbor(units, index, -1)?.type === "math") &&
      boldNeighbors.length === 0) {
      unit.math = true;
      unit.seed = true;
    }
  });

  let changed = true;
  while (changed) {
    changed = false;
    units.forEach((unit, index) => {
      if (unit.math || unit.type === "space" || unit.type === "break") return;
      const left = neighbor(units, index, -1);
      const right = neighbor(units, index, 1);
      const tightLeft = tightNeighbor(units, index, -1);
      const tightRight = tightNeighbor(units, index, 1);
      let math = false;
      if (unit.type === "punct" || unit.type === "number") {
        const text = unit.text;
        if (tightLeft?.math) {
          const followedByText = !tightRight || tightRight.type === "word";
          if (SENTENCE_PUNCTUATION.has(text) && (!right || !right.math || !tightRight)) math = false;
          else if ((text === "'" || text === "’" || text === "-") && tightRight?.type === "word") math = false;
          else if (OPENERS.has(text) && followedByText && !tightRight) math = false;
          else math = true;
        }
        if (!math && tightRight?.math && !(text === "-" && tightLeft?.type === "word") && !SENTENCE_PUNCTUATION.has(text)) {
          math = !(CLOSERS.has(text) && !tightLeft);
        }
        if (!math && isOperatorUnit(unit) && near(unit, left) && near(unit, right)) {
          const mathSide = (left?.math && (right?.math || right?.type === "number" || OPENERS.has(right?.text))) ||
            (right?.math && (left?.math || left?.type === "number" || CLOSERS.has(left?.text)));
          if (mathSide && !(unit.text === "-" && (!left?.math || !right?.math))) math = true;
        }
        if (!math && unit.type === "number" && ((near(unit, left) && isMathOperator(left)) || (near(unit, right) && isMathOperator(right)))) {
          math = true;
        }
      } else if (unit.type === "word") {
        const name = OPERATOR_NAMES.has(unit.text) || EXTRA_OPERATOR_NAMES.has(unit.text);
        if (name && ((near(unit, right) && (right.math || OPENERS.has(right.text) || right.type === "number")) ||
          (near(unit, left) && isMathOperator(left)))) {
          math = true;
        } else if (tightRight?.seed && (unit.text.length === 1 || name)) {
          math = true;
        } else if ((tightLeft?.seed || tightRight?.seed) && tightRight && (tightRight.math || OPENERS.has(tightRight.text))) {
          math = true;
        }
      }
      if (math) {
        unit.math = true;
        changed = true;
      }
    });
  }
  return units;
}

function bracketDepth(units) {
  let depth = 0;
  let minimum = 0;
  for (const unit of units) {
    for (const node of unit.nodes) {
      const ch = node.atom.kind === "glyph" ? node.atom.ch : "";
      if (OPENERS.has(ch) || ch === "⟨") depth++;
      if (CLOSERS.has(ch) || ch === "⟩") depth--;
      minimum = Math.min(minimum, depth);
    }
  }
  return { depth, minimum };
}

// Groups consecutive math units, allowing spaces and line breaks inside a formula.
export function findChains(units) {
  const chains = [];
  let current = null;
  units.forEach((unit, index) => {
    if (unit.math) {
      if (!current) {
        current = { start: index, end: index };
        chains.push(current);
      }
      current.end = index;
    } else if (unit.type !== "space" && unit.type !== "break") {
      current = null;
    }
  });

  // Join chains separated by a comma inside an open bracket, e.g. f(x, y).
  for (let index = 0; index < chains.length - 1; index++) {
    const first = chains[index];
    const second = chains[index + 1];
    const between = units.slice(first.end + 1, second.start);
    if (between.some((unit) => unit.type === "word") || between.length > 4) continue;
    if (bracketDepth(units.slice(first.start, first.end + 1)).depth > 0 &&
      bracketDepth(units.slice(second.start, second.end + 1)).minimum < 0) {
      for (const unit of between) unit.math = true;
      first.end = second.end;
      chains.splice(index + 1, 1);
      index--;
    }
  }

  extendToClosingBrackets(units, chains);
  for (const chain of chains) {
    trimChain(units, chain);
  }
  return chains.filter((chain) => chain.start <= chain.end);
}

// Intervals such as [0, 1) and arguments such as (1 - β) close after plain numbers.
function extendToClosingBrackets(units, chains) {
  for (let index = 0; index < chains.length; index++) {
    const chain = chains[index];
    const { depth } = bracketDepth(units.slice(chain.start, chain.end + 1));
    if (depth <= 0) continue;
    let level = depth;
    for (let cursor = chain.end + 1; cursor < units.length && cursor <= chain.end + 12; cursor++) {
      const unit = units[cursor];
      if (unit.type === "word" && !unit.math) break;
      const text = unit.type === "punct" ? unit.text : "";
      if (OPENERS.has(text)) level++;
      if (CLOSERS.has(text)) level--;
      if (level < depth && level >= 0 && CLOSERS.has(text)) {
        for (let mark = chain.end + 1; mark <= cursor; mark++) units[mark].math = units[mark].type !== "space" && units[mark].type !== "break" || units[mark].math;
        chain.end = cursor;
        while (chains[index + 1] && chains[index + 1].start <= cursor) {
          chain.end = Math.max(chain.end, chains[index + 1].end);
          chains.splice(index + 1, 1);
        }
        if (level === 0) break;
      }
    }
  }
}

function trimChain(units, chain) {
  let changed = true;
  while (changed && chain.start <= chain.end) {
    changed = false;
    const last = units[chain.end];
    const first = units[chain.start];
    if (last.type === "space" || last.type === "break" ||
      (last.type === "punct" && !last.seed && (SENTENCE_PUNCTUATION.has(last.text) || last.text === "-"))) {
      last.math = false;
      chain.end--;
      changed = true;
      continue;
    }
    if (first.type === "space" || first.type === "break") {
      chain.start++;
      changed = true;
      continue;
    }
    const slice = units.slice(chain.start, chain.end + 1);
    const { depth, minimum } = bracketDepth(slice);
    if (depth > minimum && first.type === "punct" && OPENERS.has(first.text) && depth > 0 && !first.seed) {
      first.math = false;
      chain.start++;
      changed = true;
    } else if (minimum < 0 && last.type === "punct" && CLOSERS.has(last.text) && !last.seed) {
      last.math = false;
      chain.end--;
      changed = true;
    }
  }
}

// ---------------------------------------------------------------------------
// Line roles and blocks

function lineUnits(line) {
  if (!line.units) {
    line.units = markMath(buildUnits(line.nodes));
    line.chains = findChains(line.units);
  }
  return line.units;
}

function equationNumber(units, context) {
  const visible = units.filter((unit) => unit.type !== "space");
  if (visible.length < 4) return 0;
  const [open, number, close] = visible.slice(-3);
  if (open.text !== "(" || close.text !== ")" || !/^[\d.]+[a-z]?$/.test(number.text)) return 0;
  const before = visible.at(-4);
  return open.x0 - before.x1 > 2 * context.bodySize ? 3 : 0;
}

function classifyLine(line, context) {
  if (line.rotated) return "text";
  const units = lineUnits(line);
  const visible = units.filter((unit) => unit.type !== "space");
  const numbered = equationNumber(units, context);
  const body = visible.slice(0, visible.length - numbered);
  const mathCount = body.filter((unit) => unit.math || unit.type === "punct" || unit.type === "number").length;
  const words = body.filter((unit) => unit.type === "word" && !unit.math);
  const hasStructure = body.some((unit) => unit.nodes.some((node) => ["matrix", "frac", "bigop"].includes(node.atom.kind)));
  const allMath = body.length > 0 && mathCount === body.length && body.some((unit) => unit.math);
  const mostlyMath = body.length > 0 && mathCount >= 0.6 * body.length && words.length <= 3 && body.some((unit) => unit.math);
  const indent = line.x0 - context.leftMargin;
  const rightGap = context.rightMargin - line.x1;
  const textCenter = (context.leftMargin + context.rightMargin) / 2;
  const lineEnd = numbered ? body.at(-1).x1 : line.x1;
  const centered = indent > 1.5 * context.bodySize && (rightGap > 1.5 * context.bodySize || numbered) &&
    Math.abs((line.x0 + lineEnd) / 2 - textCenter) < 4 * context.bodySize;
  line.equationNumber = numbered;
  if ((allMath && (centered || (hasStructure && indent > 0.5 * context.bodySize))) || (mostlyMath && centered && hasStructure)) {
    return "display";
  }
  if (isHeading(line, context)) return "heading";
  return "text";
}

function isHeading(line, context) {
  const letters = line.atoms.filter((atom) => atom.kind === "glyph" && !atom.space && /\p{L}/u.test(atom.ch));
  if (letters.length < 2) return false;
  const size = Math.max(...letters.map((atom) => atom.size));
  const bold = letters.every((atom) => atom.font.bold);
  const ratio = size / context.bodySize;
  const short = context.rightMargin - line.x1 > 2 * context.bodySize;
  if (ratio >= 1.4) return true;
  if (bold && ratio >= 1.1) return true;
  return bold && short && line.x0 <= context.leftMargin + 3 * context.bodySize && letters.length < 80 && line.isolated;
}

function headingLevel(line, context) {
  const size = Math.max(...line.atoms.filter((atom) => !atom.space).map((atom) => atom.size));
  const ratio = size / context.bodySize;
  if (ratio >= 1.3) return 1;
  if (ratio >= 1.1) return 2;
  return 3;
}

function listLabel(line, context) {
  const units = lineUnits(line);
  const first = units[0];
  const second = units[1];
  if (!first || !second) return null;
  let label = first.text;
  let consumed = 1;
  if (first.type === "number" && units[1]?.text && /^[.)]$/.test(units[1].text) && isTight(first, units[1])) {
    label += units[1].text;
    consumed = 2;
  } else if (first.text === "(" && units[2]?.text === ")") {
    label = `(${units[1].text})`;
    consumed = 3;
  } else if (first.type === "word" && units[1]?.text === ")" && isTight(first, units[1])) {
    label += ")";
    consumed = 2;
  }
  const after = units[consumed];
  if (!after || after.type !== "space" || !LABEL_RE.test(label)) return null;
  if (units.slice(0, consumed).some((unit) => unit.seed || unit.type === "math" && !/^[•◦▪‣∙·]$/.test(unit.text))) return null;
  const textUnit = units[consumed + 1];
  if (!textUnit) return null;
  if (line.x0 > context.leftMargin + 5 * context.bodySize) return null;
  return { label, consumed, textX: textUnit.x0 };
}

export function buildBlocks(lines, context) {
  lines.forEach((line, index) => {
    const previous = lines[index - 1];
    const next = lines[index + 1];
    const gapAbove = previous ? previous.bottom - line.top : Infinity;
    const gapBelow = next ? line.bottom - next.top : Infinity;
    line.isolated = gapAbove > 0.5 * context.bodySize && gapBelow > 0.3 * context.bodySize;
    line.role = classifyLine(line, context);
    // A bare page number sits apart from the text at the top or bottom of the page.
    const visible = line.atoms.filter((atom) => !atom.space);
    const others = lines.filter((other) => other !== line);
    const lowest = others.every((other) => other.base > line.base);
    const highest = others.every((other) => other.base < line.base);
    const gap = Math.min(...others.map((other) => Math.abs(other.base - line.base)), Infinity);
    if (/^([\divxlcIVXLC]{1,6}|Page \d+( of \d+)?)$/.test(visible.map((atom) => atom.ch ?? "").join("")) &&
      (lowest || highest) && gap > 2.5 * context.bodySize) {
      line.role = "furniture";
    }
  });

  const root = [];
  let list = null;
  let item = null;
  let paragraph = null;

  function container() {
    return item ? item.blocks : root;
  }

  function closeList() {
    list = null;
    item = null;
  }

  lines.forEach((line, index) => {
    const previous = lines[index - 1];
    if (line.role === "furniture") {
      paragraph = null;
      root.push({ type: "furniture", lines: [line] });
      return;
    }
    const label = line.role === "text" ? listLabel(line, context) : null;
    if (item && !label && line.role !== "display") {
      if (line.x0 < item.textX - 0.8 * context.bodySize || line.role === "heading") closeList();
    }
    if (line.role === "heading") {
      closeList();
      const last = root.at(-1);
      if (last?.type === "heading" && previous && last.lines.at(-1) === previous &&
        Math.abs(previous.size - line.size) < 0.5 && previous.bottom - line.top < 1.2 * line.size) {
        last.lines.push(line);
      } else {
        root.push({ type: "heading", level: headingLevel(line, context), lines: [line] });
      }
      paragraph = null;
      return;
    }
    if (label) {
      const ordered = /\d/.test(label.label);
      if (!list || Math.abs(line.x0 - list.x0) > context.bodySize || list.ordered !== ordered) {
        list = { type: "list", ordered, x0: line.x0, items: [] };
        root.push(list);
      }
      item = { label: label.label, labelUnits: label.consumed, textX: label.textX, blocks: [] };
      list.items.push(item);
      paragraph = { type: "paragraph", lines: [line], label };
      item.blocks.push(paragraph);
      return;
    }
    if (line.role === "display") {
      const target = container();
      const last = target.at(-1);
      if (last?.type === "display" && previous && last.lines.at(-1) === previous &&
        previous.bottom - line.top < 1.5 * context.bodySize) {
        last.lines.push(line);
      } else {
        target.push({ type: "display", lines: [line] });
      }
      paragraph = null;
      return;
    }
    if (paragraph && previous && paragraph.lines.at(-1) === previous && !startsParagraph(previous, line, context, item)) {
      paragraph.lines.push(line);
      return;
    }
    paragraph = { type: "paragraph", lines: [line] };
    container().push(paragraph);
  });
  return root;
}

function startsParagraph(previous, line, context, item) {
  const left = item ? item.textX : context.leftMargin;
  const gap = previous.base - line.base;
  if (gap > 1.6 * context.lineSpacing || gap < -0.5 * context.bodySize) return true;
  if (context.rightMargin - previous.x1 > 2.5 * context.bodySize) return true;
  if (line.x0 - left > 0.8 * context.bodySize && Math.abs(line.x0 - previous.x0) > 0.8 * context.bodySize) return true;
  return false;
}
