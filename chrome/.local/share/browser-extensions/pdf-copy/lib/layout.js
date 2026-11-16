// Rebuilds lines, paragraphs, and math structure from positioned PDF glyphs.
import {
  ACCENTS,
  BIG_OPERATORS,
  GLYPH_PAIRS,
  MATH_SYMBOLS,
  MATRIX_ENVIRONMENTS,
  RELATIONS,
  UNICODE_PIECES,
  cmexEntry,
  controlCharacter
} from "./symbols.js";

const LIGATURES = { "ﬀ": "ff", "ﬁ": "fi", "ﬂ": "fl", "ﬃ": "ffi", "ﬄ": "ffl", "ﬅ": "st", "ﬆ": "st" };
const LIMIT_WORDS = /^(lim|max|min|sup|inf|argmax|argmin|liminf|limsup|lim\s?inf|lim\s?sup|arg\s?max|arg\s?min)$/;
const NARROW = new Set("iljtfr.,;:!'`|()[]{}/\\’‘");

function charWeight(char) {
  if (/^\s+$/u.test(char)) return 0.33;
  if (/^\p{M}+$/u.test(char)) return 0;
  if (NARROW.has(char)) return "tfr".includes(char) ? 0.38 : 0.28;
  if ("mwMW".includes(char)) return 0.85;
  if (/[A-Z]/.test(char)) return 0.72;
  if (/[0-9a-z]/.test(char)) return 0.5;
  if ("=+<>−±×÷≤≥≠≈∼→←⇒⇐↔∈∉⊂⊃⊆⊇∪∩".includes(char)) return 0.78;
  return 0.6;
}

function isRadicalGlyph(glyph) {
  return glyph.ch === "√" || glyph.ext?.kind === "radical";
}

// ---------------------------------------------------------------------------
// Glyphs

function buildGlyphs(data) {
  const glyphs = [];
  data.items.forEach((item, itemIndex) => {
    if (!item.str || item.font.math === "extension") return;
    const chars = Array.from(item.str);
    const weights = chars.map(charWeight);
    const total = weights.reduce((sum, weight) => sum + weight, 0) || 1;
    let x = item.x;
    let offset = 0;
    chars.forEach((raw, index) => {
      const width = item.horizontal ? (item.width * weights[index]) / total : 0;
      const ch = LIGATURES[raw] ?? (/^[\x00-\x1f]$/.test(raw) ? controlCharacter(raw, item.font) : null) ?? raw;
      const glyph = {
        kind: "glyph",
        ch,
        item: itemIndex,
        offset,
        length: raw.length,
        font: item.font,
        size: item.size,
        x0: x,
        x1: x + width,
        y: item.y,
        base: item.y,
        space: /^\s+$/u.test(ch),
        rotated: !item.horizontal,
        order: itemIndex + index / (chars.length + 1)
      };
      setVerticalExtent(glyph);
      glyphs.push(glyph);
      x += width;
      offset += raw.length;
    });
  });

  const extensionItems = data.items
    .map((item, index) => ({ item, index }))
    .filter(({ item }) => item.font.math === "extension");
  for (const source of data.extensionGlyphs) {
    const entry = extensionEntry(source);
    if (!entry) continue;
    const match = extensionItems.find(({ item }) =>
      source.x >= item.x - 0.6 && source.x <= item.x + item.width + 0.6 && Math.abs(item.y - source.y) < 0.6
    );
    const glyph = {
      kind: "glyph",
      ch: "",
      ext: entry,
      item: match ? match.index : -1,
      offset: 0,
      length: match ? match.item.str.length : 0,
      font: source.font,
      size: source.size,
      x0: source.x,
      x1: source.x + source.width,
      y: source.y,
      top: entry.kind === "accent" ? source.y + 0.8 * source.size : source.y + 0.05 * source.size,
      bottom: entry.kind === "accent" ? source.y + 0.5 * source.size : source.y - entry.size * source.size,
      space: false,
      rotated: false,
      order: match ? match.index + (0.9 * (source.x - match.item.x)) / Math.max(match.item.width, 1) : NaN
    };
    glyph.base = entry.kind === "accent" ? source.y : (glyph.top + glyph.bottom) / 2 - 0.25 * glyph.size;
    glyphs.push(glyph);
  }
  assignMissingOrder(glyphs);
  return dedupe(glyphs);
}

const CMEX_COMPATIBLE = /^(cmex|lmex|lmmathextension|eurm?ex|euex|mathx)/i;
const MOSTLY_CMEX = /^(txex|pxex|ntxex|zxex)/i;

function extensionEntry(source) {
  const name = source.font.name;
  if (CMEX_COMPATIBLE.test(name) || (MOSTLY_CMEX.test(name) && source.code < 0x80)) {
    const entry = cmexEntry(source.code);
    if (entry) return entry;
  }
  const char = Array.from(source.unicode.replace(/[\ufe00-\ufe0f]/g, ""))[0] || "";
  const latex = MATH_SYMBOLS.get(char) ?? char;
  const width = source.width / source.size;
  if (BIG_OPERATORS.has(latex)) {
    const integral = latex.includes("int");
    return { kind: "operator", latex, size: integral ? (width > 0.5 ? 2.22 : 1.11) : width > 1.2 ? 1.4 : 1.0 };
  }
  const piece = UNICODE_PIECES.get(char);
  if (piece) return { kind: "piece", latex: piece[0], part: piece[1], size: piece[1] === "ex" ? 0.6 : 1.2 };
  if (["(", ")", "[", "]", "\\{", "\\}", "\\langle", "\\rangle", "\\lfloor", "\\rfloor", "\\lceil", "\\rceil", "|", "\\|"].includes(latex)) {
    return { kind: "delimiter", latex, size: 1.8, side: ["(", "[", "\\{", "\\langle", "\\lfloor", "\\lceil"].includes(latex) ? "left" : latex === "|" || latex === "\\|" ? "both" : "right" };
  }
  if (char === "√") return { kind: "radical", size: 1.8 };
  return null;
}

function setVerticalExtent(glyph) {
  const { size, y } = glyph;
  const piece = UNICODE_PIECES.get(glyph.ch);
  if (piece) {
    glyph.top = y + 0.75 * size;
    glyph.bottom = y - 0.25 * size;
    glyph.piece = piece;
  } else if (glyph.ch === "√" && glyph.font.math === "symbol") {
    glyph.top = y + 0.05 * size;
    glyph.bottom = y - 0.96 * size;
    glyph.base = (glyph.top + glyph.bottom) / 2 - 0.25 * size;
  } else {
    glyph.top = y + 0.72 * size;
    glyph.bottom = y - 0.22 * size;
  }
}

function assignMissingOrder(glyphs) {
  const ordered = glyphs.filter((glyph) => !Number.isNaN(glyph.order) && !glyph.space);
  for (const glyph of glyphs) {
    if (!Number.isNaN(glyph.order)) continue;
    let best = null;
    let bestDistance = Infinity;
    const center = (glyph.top + glyph.bottom) / 2;
    for (const other of ordered) {
      const dx = Math.max(0, other.x0 - glyph.x1, glyph.x0 - other.x1);
      const dy = Math.abs((other.top + other.bottom) / 2 - center);
      const distance = dx + dy * 0.6;
      if (distance < bestDistance) {
        bestDistance = distance;
        best = other;
      }
    }
    glyph.order = best ? best.order + (glyph.x0 >= best.x0 ? 1e-4 : -1e-4) : 0;
  }
}

// Some PDFs draw text twice for fake bold or shadows.
function dedupe(glyphs) {
  const seen = new Map();
  return glyphs.filter((glyph) => {
    if (glyph.space) return true;
    const key = `${glyph.ch}|${glyph.ext ? glyph.font.name : glyph.font.name}|${Math.round(glyph.y * 2)}`;
    const list = seen.get(key) || [];
    if (list.some((other) => Math.abs(other.x0 - glyph.x0) < 0.4 && Math.abs(other.y - glyph.y) < 0.4 && other.ext === glyph.ext)) {
      return false;
    }
    list.push(glyph);
    seen.set(key, list);
    return true;
  });
}

function estimateBodySize(glyphs) {
  const counts = new Map();
  for (const glyph of glyphs) {
    if (glyph.space || glyph.ext || glyph.font.math) continue;
    const key = Math.round(glyph.size * 10) / 10;
    counts.set(key, (counts.get(key) || 0) + 1);
  }
  let best = 10;
  let bestCount = 0;
  for (const [size, count] of counts) {
    if (count > bestCount) {
      best = size;
      bestCount = count;
    }
  }
  return best;
}

// ---------------------------------------------------------------------------
// Composite atoms

function makeAtom(kind, members, props = {}) {
  const leaves = [];
  for (const member of members) {
    if (!member) continue;
    if (member.kind === "glyph") leaves.push(member);
    else leaves.push(...member.glyphs);
  }
  const visible = members.filter((member) => member && !member.space);
  const atom = {
    kind,
    glyphs: leaves,
    x0: Math.min(...visible.map((member) => member.x0)),
    x1: Math.max(...visible.map((member) => member.x1)),
    top: Math.max(...visible.map((member) => member.top)),
    bottom: Math.min(...visible.map((member) => member.bottom)),
    size: Math.max(...visible.map((member) => member.size)),
    order: Math.min(...visible.map((member) => member.order)),
    rotated: false,
    ...props
  };
  if (atom.base === undefined) atom.base = atom.bottom + 0.22 * atom.size;
  return atom;
}

// Finds the baseline of the surrounding text line at a vertical position.
function neighborBaseline(atoms, exclude, x0, x1, y, size) {
  let best = null;
  let bestDistance = 4 * size;
  for (const atom of atoms) {
    if (exclude.has(atom) || atom.space || atom.consumed) continue;
    if (atom.bottom > y || atom.top < y) continue;
    if (atom.ext && atom.ext.kind !== "operator") continue;
    const distance = Math.max(0, atom.x0 - x1, x0 - atom.x1);
    if (distance < bestDistance && atom.size >= 0.85 * size) {
      bestDistance = distance;
      best = atom;
    }
  }
  return best;
}

function within(atom, x0, x1, tolerance = 1.5) {
  return atom.x0 >= x0 - tolerance && atom.x1 <= x1 + tolerance;
}

function horizontalExtent(atoms) {
  const visible = atoms.filter((atom) => !atom.space);
  return visible.length
    ? [Math.min(...visible.map((atom) => atom.x0)), Math.max(...visible.map((atom) => atom.x1))]
    : [0, 0];
}

// Keeps the atoms on the baseline closest to a reference line, plus their scripts.
function nearestBand(candidates, distance, maxGap) {
  if (!candidates.length) return [];
  const sorted = [...candidates].sort((a, b) => distance(a) - distance(b));
  const nearest = sorted[0];
  const limit = distance(nearest) + maxGap * nearest.size;
  return sorted.filter((atom) => distance(atom) <= limit && Math.abs(atom.base - nearest.base) < 0.6 * nearest.size);
}

function buildDots(atoms) {
  const dots = atoms.filter((atom) => atom.kind === "glyph" && (atom.ch === "." || atom.ch === "·") && !atom.consumed)
    .sort((a, b) => a.y - b.y);
  const result = [];
  for (const dot of dots) {
    if (dot.consumed) continue;
    const step = 0.3 * dot.size;
    // Seen from the lowest dot, \ddots continues up and to the left.
    for (const [dx, command] of [[0, "\\vdots"], [-step, "\\ddots"], [step, "\\iddots"]]) {
      const second = dots.find((other) => !other.consumed && other !== dot && other.ch === dot.ch && other.font === dot.font &&
        Math.abs(other.x0 - dot.x0 - dx) < 0.18 * dot.size + Math.abs(dx) * 0.6 &&
        other.y - dot.y > 0.2 * dot.size && other.y - dot.y < 0.55 * dot.size &&
        (dx === 0 ? Math.abs(other.x0 - dot.x0) < 0.12 * dot.size : Math.sign(other.x0 - dot.x0) === Math.sign(dx)));
      if (!second) continue;
      const third = dots.find((other) => !other.consumed && other !== dot && other !== second &&
        other.ch === dot.ch && other.font === dot.font &&
        Math.abs(other.x0 - second.x0 - (second.x0 - dot.x0)) < 0.06 * dot.size &&
        Math.abs(other.y - second.y - (second.y - dot.y)) < 0.06 * dot.size);
      if (!third) continue;
      const lowest = dot;
      for (const member of [dot, second, third]) member.consumed = true;
      result.push(makeAtom("symbol", [dot, second, third], {
        latex: command,
        base: lowest.y,
        size: dot.size,
        top: lowest.y + 0.75 * dot.size,
        bottom: lowest.y - 0.1 * dot.size
      }));
      break;
    }
  }
  return result;
}

function buildAccents(atoms) {
  const result = [];
  const accents = atoms.filter((atom) => atom.kind === "glyph" && !atom.consumed &&
    (ACCENTS.has(atom.ch) || atom.ext?.kind === "accent"));
  for (const accent of accents) {
    if (accent.consumed) continue;
    // Combining accents have no advance and are drawn to the left of their origin.
    const center = accent.x1 - accent.x0 < 0.05 * accent.size ? accent.x0 - 0.25 * accent.size : (accent.x0 + accent.x1) / 2;
    const wide = accent.ext?.kind === "accent";
    const bases = atoms.filter((atom) => atom !== accent && !atom.consumed && !atom.space &&
      !ACCENTS.has(atom.ch) && atom.kind === "glyph" && !atom.ext && /^[\p{L}\p{N}]$/u.test(atom.ch) &&
      atom.base <= accent.y + 0.1 * accent.size && accent.y - atom.base < (wide ? 1.2 : 0.6) * atom.size &&
      (wide
        ? Math.min(atom.x1, accent.x1) - Math.max(atom.x0, accent.x0) > 0.4 * (atom.x1 - atom.x0)
        : center >= atom.x0 - 0.12 * atom.size && center <= atom.x1 + 0.12 * atom.size));
    if (!bases.length) continue;
    // A narrow accent belongs to the single base it is centered over.
    const chosen = wide
      ? bases.filter((atom) => Math.abs(atom.base - bases[0].base) < 0.2 * atom.size)
      : [bases.sort((a, b) => Math.abs((a.x0 + a.x1) / 2 - center) - Math.abs((b.x0 + b.x1) / 2 - center))[0]];
    accent.consumed = true;
    for (const base of chosen) base.consumed = true;
    const baseAtom = chosen.sort((a, b) => a.x0 - b.x0);
    result.push(makeAtom("accent", [accent, ...baseAtom], {
      accent: wide ? accent.ext.latex : ACCENTS.get(accent.ch),
      accentChar: accent.ch,
      body: baseAtom,
      base: baseAtom[0].base,
      size: baseAtom[0].size,
      font: baseAtom[0].font,
      order: Math.min(accent.order, ...baseAtom.map((atom) => atom.order))
    }));
  }
  return result;
}

function processRules(atoms, rules, earlier = []) {
  const created = [...earlier];
  const horizontal = rules
    .filter((rule) => rule.y1 - rule.y0 <= 1.6 && rule.x1 - rule.x0 >= 1.2)
    .sort((a, b) => (a.x1 - a.x0) - (b.x1 - b.x0));
  for (const rule of horizontal) {
    const live = atoms.filter((atom) => !atom.consumed).concat(created.filter((atom) => !atom.consumed));
    const atom = radicalForRule(rule, live) || fractionForRule(rule, live) ||
      lineForRule(rule, live) || underscoreForRule(rule, live);
    if (atom) created.push(atom);
  }
  return created.slice(earlier.length);
}

function radicalForRule(rule, atoms) {
  const mid = (rule.y0 + rule.y1) / 2;
  const radical = atoms.find((atom) => atom.kind === "glyph" && isRadicalGlyph(atom) && Math.abs(atom.x1 - rule.x0) < 2 &&
    (atom.ext || atom.font.math === "symbol"
      ? Math.abs(mid - atom.top) < 0.12 * (atom.top - atom.bottom) + 1.5
      : mid > atom.y && mid - atom.y < 3 * atom.size));
  if (!radical) return null;
  const known = Boolean(radical.ext || radical.font.math === "symbol");
  const body = atoms.filter((atom) => atom !== radical && !atom.space &&
    atom.x0 >= rule.x0 - 1 && atom.x1 <= rule.x1 + 1.5 &&
    atom.top <= mid + 0.25 * atom.size && mid - atom.top < 1.2 * Math.max(atom.size, 0.7 * radical.size) &&
    (!known || atom.bottom >= radical.bottom - 0.4 * radical.size));
  if (!body.length) return null;
  const index = atoms.filter((atom) => atom !== radical && !atom.space && !body.includes(atom) &&
    atom.size < 0.8 * radical.size && atom.x1 <= radical.x0 + 0.7 * (radical.x1 - radical.x0) &&
    atom.x0 >= radical.x0 - 1.5 * radical.size && atom.base > radical.bottom + 0.3 * (radical.top - radical.bottom) &&
    atom.top <= mid + 1);
  const main = mainAtoms(body);
  for (const atom of [radical, ...body, ...index]) atom.consumed = true;
  return makeAtom("sqrt", [radical, ...body, ...index], {
    body,
    index: index.length ? index : null,
    base: main[0]?.base ?? radical.base,
    size: Math.max(...main.map((atom) => atom.size))
  });
}

function mainAtoms(atoms) {
  const visible = atoms.filter((atom) => !atom.space);
  const size = Math.max(...visible.map((atom) => atom.size));
  return visible.filter((atom) => atom.size >= 0.88 * size).sort((a, b) => a.base - b.base);
}

function fractionForRule(rule, atoms) {
  const mid = (rule.y0 + rule.y1) / 2;
  const width = rule.x1 - rule.x0;
  if (width < 2) return null;
  const candidates = atoms.filter((atom) => !atom.space && within(atom, rule.x0, rule.x1) && !atom.rotated);
  const above = candidates.filter((atom) => atom.base > mid + 0.05 * atom.size && atom.bottom >= mid - 0.35 * atom.size &&
    atom.bottom - mid < 1.6 * atom.size);
  const below = candidates.filter((atom) => atom.top < mid + 0.35 * atom.size && atom.base < mid &&
    mid - atom.top < 1.6 * atom.size);
  const numerator = nearestBand(above, (atom) => atom.bottom - mid, 0.9);
  const denominator = nearestBand(below, (atom) => mid - atom.top, 0.9);
  if (!numerator.length || !denominator.length) return null;
  // The parts nearest to the rule must touch it; scripts may reach further.
  if (Math.min(...numerator.map((atom) => (atom.bottom - mid) / atom.size)) > 0.9) return null;
  if (Math.min(...denominator.map((atom) => (mid - atom.top) / atom.size)) > 1.0) return null;
  const [n0, n1] = horizontalExtent(numerator);
  const [d0, d1] = horizontalExtent(denominator);
  const contentWidth = Math.max(n1 - n0, d1 - d0);
  const tolerance = Math.max(4, 0.35 * Math.min(...numerator.concat(denominator).map((atom) => atom.size)));
  if (contentWidth < width - tolerance || contentWidth > width + 3) return null;
  const center = (rule.x0 + rule.x1) / 2;
  if (Math.abs((n0 + n1) / 2 - center) > 0.25 * width + 1.5) return null;
  if (Math.abs((d0 + d1) / 2 - center) > 0.25 * width + 1.5) return null;
  // Numerators and denominators stand alone; running text continues beside a line of prose.
  for (const part of [numerator, denominator]) {
    const base = part[0].base;
    const proseBeside = atoms.some((atom) => !part.includes(atom) && atom.kind === "glyph" && !atom.space &&
      !atom.font.math && /\p{L}/u.test(atom.ch) && Math.abs(atom.base - base) < 0.2 * atom.size &&
      (atom.x1 <= rule.x0 + 0.5 || atom.x0 >= rule.x1 - 0.5) && Math.max(rule.x0 - atom.x1, atom.x0 - rule.x1) < atom.size);
    if (proseBeside) return null;
  }
  const members = [...numerator, ...denominator];
  for (const atom of members) atom.consumed = true;
  const size = Math.max(...members.map((atom) => atom.size));
  const neighbor = neighborBaseline(atoms, new Set(members), rule.x0, rule.x1, mid, size * 0.7);
  return makeAtom("frac", members, {
    numerator,
    denominator,
    base: neighbor ? neighbor.base : mid - 0.25 * size / 0.7,
    size: neighbor ? Math.max(neighbor.size, size) : size / 0.7,
    top: Math.max(...numerator.map((atom) => atom.top)),
    bottom: Math.min(...denominator.map((atom) => atom.bottom))
  });
}

function lineForRule(rule, atoms) {
  const mid = (rule.y0 + rule.y1) / 2;
  const width = rule.x1 - rule.x0;
  const candidates = atoms.filter((atom) => !atom.space && within(atom, rule.x0, rule.x1, Math.max(1, 0.25 * atom.size)));
  const under = candidates.filter((atom) => atom.top <= mid + 0.4 * atom.size && mid - atom.top < 0.45 * atom.size + 1);
  const over = candidates.filter((atom) => atom.base > mid && atom.base - mid < 0.45 * atom.size);
  for (const [group, kind] of [[under, "overline"], [over, "underline"]]) {
    if (!group.length) continue;
    const band = nearestBand(group, (atom) => Math.abs(atom.base - group[0].base), 0.3);
    const textLetters = band.filter((atom) => atom.kind === "glyph" && !atom.font.math && /\p{L}/u.test(atom.ch)).length;
    if (textLetters > 3) continue;
    const [x0, x1] = horizontalExtent(band);
    if (Math.abs((x1 - x0) - width) > Math.max(3, 0.25 * width)) continue;
    for (const atom of band) atom.consumed = true;
    const main = mainAtoms(band);
    return makeAtom(kind, band, { body: band, base: main[0].base, size: main[0].size });
  }
  return null;
}

// TeX draws \_ as a short rule between two letters.
function underscoreForRule(rule, atoms) {
  const mid = (rule.y0 + rule.y1) / 2;
  const width = rule.x1 - rule.x0;
  const left = atoms.filter((atom) => atom.kind === "glyph" && !atom.space && !atom.font.math &&
    atom.x1 <= rule.x0 + 3 && rule.x0 - atom.x1 < 5 && mid - atom.base > -0.1 * atom.size && mid - atom.base < 0.25 * atom.size)
    .sort((a, b) => b.x1 - a.x1)[0];
  if (!left || width > 0.8 * left.size) return null;
  const center = (rule.x0 + rule.x1) / 2;
  const space = atoms.filter((atom) => atom.space && atom.item === left.item && Math.abs(atom.base - left.base) < 0.2 * left.size &&
    Math.abs((atom.x0 + atom.x1) / 2 - center) < 5).sort((a, b) =>
    Math.abs((a.x0 + a.x1) / 2 - center) - Math.abs((b.x0 + b.x1) / 2 - center))[0];
  if (space) {
    space.ch = "_";
    space.space = false;
    return null;
  }
  return {
    ...left,
    ch: "_",
    synthetic: true,
    item: -1,
    x0: rule.x0,
    x1: rule.x1,
    order: left.order + 1e-4,
    glyphs: undefined,
    neighbor: left
  };
}

function buildLimits(atoms) {
  const result = [];
  const operators = [];
  for (const atom of atoms) {
    if (atom.consumed || atom.kind !== "glyph") continue;
    if (atom.ext?.kind === "operator") operators.push({ members: [atom], latex: atom.ext.latex });
    else if (atom.font.math && BIG_OPERATORS.has(MATH_SYMBOLS.get(atom.ch))) {
      operators.push({ members: [atom], latex: MATH_SYMBOLS.get(atom.ch) });
    }
  }
  // Operator names such as lim and max also take limits in display style.
  const words = romanWords(atoms.filter((atom) => !atom.consumed));
  for (const word of words) {
    if (LIMIT_WORDS.test(word.text)) operators.push({ members: word.glyphs, latex: `\\${word.text.replace(/\s/g, "")}`, word: true });
  }
  for (const operator of operators) {
    const [x0, x1] = horizontalExtent(operator.members);
    const top = Math.max(...operator.members.map((atom) => atom.top));
    const bottom = Math.min(...operator.members.map((atom) => atom.bottom));
    const size = Math.max(...operator.members.map((atom) => atom.size));
    const reach = operator.word ? 0.45 : 0.6;
    const candidates = atoms.filter((atom) => !atom.consumed && !atom.space && !operator.members.includes(atom) &&
      glyphSize(atom) < 0.9 * size && atom.x1 > x0 - 0.5 * size && atom.x0 < x1 + 0.9 * size);
    const base = operator.members[0].base;
    // Display integrals push the upper limit right by their italic correction.
    const besideRight = (atom, reachRight) => !operator.word && atom.x0 >= x0 + 0.3 * (x1 - x0) && atom.x0 <= x1 + reachRight * size;
    const neighbors = atoms.filter((atom) => !atom.consumed && !atom.space && !operator.members.includes(atom) &&
      glyphSize(atom) < 0.9 * size && atom.x1 > x0 - 3 * size && atom.x0 < x1 + 3 * size);
    const upper = grow(candidates.filter((atom) =>
      (atom.bottom >= top - 0.35 * size && atom.bottom - top < reach * size && centerWithin(atom, x0, x1, size)) ||
      (besideRight(atom, 0.7) && atom.base > base + 0.45 * size && atom.bottom <= top + 0.6 * size)), neighbors);
    const lower = grow(candidates.filter((atom) =>
      (atom.top <= bottom + 0.35 * size && bottom - atom.top < reach * size &&
        atom.base < (operator.word ? base - 0.3 * size : bottom + 0.2 * size) && centerWithin(atom, x0, x1, size)) ||
      (besideRight(atom, 0.3) && atom.base < base - 0.25 * size && atom.top >= bottom - 0.6 * size)), neighbors);
    if (!upper.length && !lower.length) continue;
    const members = [...operator.members, ...upper, ...lower];
    for (const atom of members) atom.consumed = true;
    const op = operator.members[0];
    result.push(makeAtom("bigop", members, {
      latex: operator.latex,
      upper,
      lower,
      base: operator.word ? op.base : op.base,
      size
    }));
  }
  return result;
}

function glyphSize(atom) {
  return atom.kind === "glyph" ? atom.size : Math.max(...atom.glyphs.map((glyph) => glyph.size));
}

function centerWithin(atom, x0, x1, size) {
  const center = (atom.x0 + atom.x1) / 2;
  return center >= x0 - 0.2 * size && center <= x1 + 0.2 * size;
}

// Extends a set of limit atoms to neighbors on the same baseline.
function grow(seed, candidates) {
  const result = [...seed];
  let changed = result.length > 0;
  while (changed) {
    changed = false;
    for (const atom of candidates) {
      if (result.includes(atom)) continue;
      if (result.some((other) => Math.abs(other.base - atom.base) < 0.25 * other.size &&
        Math.max(other.x0 - atom.x1, atom.x0 - other.x1) < 0.35 * other.size)) {
        result.push(atom);
        changed = true;
      }
    }
  }
  return result;
}

function romanWords(atoms) {
  const letters = atoms.filter((atom) => atom.kind === "glyph" && !atom.font.math && /^[a-zA-Z]$/.test(atom.ch))
    .sort((a, b) => a.order - b.order);
  const words = [];
  let current = null;
  for (const glyph of letters) {
    if (current && glyph.item === current.glyphs.at(-1).item && glyph.offset === current.end) {
      current.glyphs.push(glyph);
      current.text += glyph.ch;
      current.end = glyph.offset + glyph.length;
    } else {
      current = { glyphs: [glyph], text: glyph.ch, end: glyph.offset + glyph.length };
      words.push(current);
    }
  }
  return words;
}

// Joins extensible delimiter pieces into tall delimiters.
function buildDelimiters(atoms) {
  const pieces = atoms.filter((atom) => atom.kind === "glyph" && !atom.consumed &&
    (atom.ext?.kind === "piece" || atom.ext?.kind === "delimiter" || atom.piece));
  const delimiters = [];
  const used = new Set();
  for (const piece of pieces.sort((a, b) => b.top - a.top)) {
    if (used.has(piece)) continue;
    const latex = piece.ext ? piece.ext.latex : piece.piece[0];
    const group = [piece];
    used.add(piece);
    if (piece.ext?.kind !== "delimiter") {
      let current = piece;
      for (;;) {
        const next = pieces.find((other) => !used.has(other) && other.ext?.kind !== "delimiter" &&
          (other.ext ? other.ext.latex : other.piece[0]) === latex &&
          Math.abs(other.x0 - current.x0) < 0.6 && other.top <= current.bottom + 0.3 * current.size &&
          other.top >= current.bottom - 0.6 * current.size);
        if (!next) break;
        group.push(next);
        used.add(next);
        current = next;
      }
    }
    const top = Math.max(...group.map((atom) => atom.top));
    const bottom = Math.min(...group.map((atom) => atom.bottom));
    const size = Math.max(...group.map((atom) => atom.size));
    delimiters.push({
      members: group,
      latex,
      side: latex === "|" || latex === "\\|" ? "both" : piece.ext?.side ||
        (["(", "[", "\\{", "\\langle", "\\lceil", "\\lfloor"].includes(latex) ? "left" : "right"),
      top,
      bottom,
      x0: Math.min(...group.map((atom) => atom.x0)),
      x1: Math.max(...group.map((atom) => atom.x1)),
      size,
      base: (top + bottom) / 2 - 0.25 * size
    });
  }
  return delimiters;
}

function verticalOverlap(a, b) {
  const overlap = Math.min(a.top, b.top) - Math.max(a.bottom, b.bottom);
  return overlap / Math.min(a.top - a.bottom, b.top - b.bottom);
}

function pairDelimiters(delimiters) {
  const pairs = [];
  const stack = [];
  for (const delimiter of [...delimiters].sort((a, b) => a.x0 - b.x0)) {
    const matchIndex = (() => {
      for (let index = stack.length - 1; index >= 0; index--) {
        const open = stack[index];
        if (verticalOverlap(open, delimiter) < 0.6) continue;
        const height = delimiter.top - delimiter.bottom;
        if (Math.abs(open.top - open.bottom - height) > 0.35 * height + 1) continue;
        if (delimiter.side === "right" || (delimiter.side === "both" && open.latex === delimiter.latex)) return index;
      }
      return -1;
    })();
    if (matchIndex >= 0) {
      const open = stack[matchIndex];
      stack.splice(matchIndex, 1);
      pairs.push({ open, close: delimiter });
    } else if (delimiter.side !== "right") {
      stack.push(delimiter);
    } else {
      pairs.push({ open: null, close: delimiter });
    }
  }
  for (const open of stack) pairs.push({ open, close: null });
  return pairs;
}

function clusterRows(atoms) {
  const main = mainAtoms(atoms).sort((a, b) => b.base - a.base);
  const rows = [];
  for (const atom of main) {
    const row = rows.find((candidate) => Math.abs(candidate.base - atom.base) < 0.45 * atom.size);
    if (row) row.atoms.push(atom);
    else rows.push({ base: atom.base, size: atom.size, atoms: [atom] });
  }
  for (const atom of atoms) {
    if (atom.space || main.includes(atom)) continue;
    let best = null;
    for (const row of rows) {
      if (!best || Math.abs(row.base - atom.base) < Math.abs(best.base - atom.base)) best = row;
    }
    if (best && Math.abs(best.base - atom.base) < 0.8 * best.size) best.atoms.push(atom);
  }
  return rows.sort((a, b) => b.base - a.base);
}

function splitColumns(rows, size) {
  const intervals = rows.flatMap((row) => row.atoms.filter((atom) => !atom.space).map((atom) => [atom.x0, atom.x1]))
    .sort((a, b) => a[0] - b[0]);
  const columns = [];
  for (const [x0, x1] of intervals) {
    const last = columns.at(-1);
    if (last && x0 - last[1] < 0.55 * size) last[1] = Math.max(last[1], x1);
    else columns.push([x0, x1]);
  }
  return rows.map((row) => columns.map(([x0, x1]) => row.atoms.filter((atom) => {
    const center = (atom.x0 + atom.x1) / 2;
    return center >= x0 - 0.5 && center <= x1 + 0.5;
  })));
}

function buildDelimiterGroups(atoms, extra) {
  const live = () => atoms.concat(extra).filter((atom) => !atom.consumed);
  const delimiters = buildDelimiters(live());
  const pairs = pairDelimiters(delimiters)
    .sort((a, b) => spanWidth(a) - spanWidth(b));
  const result = [];
  const rightEdge = Math.max(...live().map((atom) => atom.x1));
  for (const { open, close } of pairs) {
    const reference = open || close;
    if (reference.top - reference.bottom < 1.1 * reference.size && !(open && close)) continue;
    const x0 = open ? open.x1 : -Infinity;
    const x1 = close ? close.x0 : open.latex === "\\{" ? rightEdge : -Infinity;
    if (x1 === -Infinity) continue;
    const members = new Set([...(open?.members || []), ...(close?.members || [])]);
    const content = live().filter((atom) => !members.has(atom) && !atom.space &&
      atom.x0 >= x0 - 0.5 && atom.x1 <= x1 + 0.5 &&
      (atom.top + atom.bottom) / 2 <= reference.top + 0.5 && (atom.top + atom.bottom) / 2 >= reference.bottom - 0.5);
    if (!content.length) continue;
    const rows = clusterRows(content);
    const tall = reference.top - reference.bottom > 1.5 * reference.size;
    for (const atom of [...members, ...content]) atom.consumed = true;
    const all = [...members, ...content];
    const base = reference.base;
    if (rows.length >= 2 && tall) {
      const environment = matrixEnvironment(open, close);
      const wrapped = environment === "matrix" && Boolean(open || close);
      result.push(makeAtom("matrix", all, {
        environment,
        open: wrapped ? open?.latex || "." : null,
        close: wrapped ? close?.latex || "." : null,
        cells: splitColumns(rows, reference.size),
        base,
        size: reference.size,
        top: reference.top,
        bottom: reference.bottom
      }));
    } else {
      result.push(makeAtom("leftright", all, {
        open: open?.latex || ".",
        close: close?.latex || ".",
        body: content,
        base: mainAtoms(content)[0]?.base ?? base,
        size: Math.max(...content.map((atom) => atom.size))
      }));
    }
  }
  return result;
}

const MATCHING_CLOSER = new Map(Object.entries({
  "(": ")", "[": "]", "\\{": "\\}", "|": "|", "\\|": "\\|", "\\langle": "\\rangle",
  "\\lfloor": "\\rfloor", "\\lceil": "\\rceil"
}));

function matrixEnvironment(open, close) {
  if (open && !close && open.latex === "\\{") return "cases";
  if (open && close && MATCHING_CLOSER.get(open.latex) === close.latex) {
    return MATRIX_ENVIRONMENTS.get(open.latex) || "matrix";
  }
  return "matrix";
}

function spanWidth({ open, close }) {
  if (open && close) return close.x0 - open.x1;
  return 1e6;
}

const GLYPH_CLOSERS = new Map(Object.entries({ "(": ")", "[": "]", "{": "}", "⟨": "⟩", "|": "|", "‖": "‖", "∥": "∥" }));
const GLYPH_DELIMITERS = new Map(Object.entries({ "(": "(", "[": "[", "{": "\\{", "⟨": "\\langle", "|": "|", "‖": "\\|", "∥": "\\|" }));

function buildGlyphMatrices(atoms) {
  const result = [];
  const delimiters = atoms.filter((atom) => atom.kind === "glyph" && !atom.consumed && !atom.ext &&
    atom.font.math === "unicode" && /^[()[\]{}⟨⟩|‖∥]$/.test(atom.ch)).sort((a, b) => a.x0 - b.x0);
  const stack = [];
  for (const delimiter of delimiters) {
    const sameLine = (other) => Math.abs(other.base - delimiter.base) < 0.3 * delimiter.size;
    const openIndex = stack.findLastIndex((open) => sameLine(open) && GLYPH_CLOSERS.get(open.ch) === delimiter.ch);
    if (openIndex < 0) {
      if (GLYPH_CLOSERS.has(delimiter.ch)) stack.push(delimiter);
      continue;
    }
    const [open] = stack.splice(openIndex, 1);
    const close = delimiter;
    const axis = open.base + 0.25 * open.size;
    const content = atoms.filter((atom) => !atom.consumed && atom !== open && atom !== close && !atom.space &&
      atom.x0 >= open.x1 - 0.5 && atom.x1 <= close.x0 + 0.5 && Math.abs((atom.top + atom.bottom) / 2 - axis) < 3 * open.size);
    // Lines of running text pass beyond the delimiters; matrix rows do not.
    const passesThrough = (row) => Math.abs(row.base - open.base) > 0.4 * open.size && atoms.some((atom) =>
      !atom.consumed && !atom.space && Math.abs(atom.base - row.base) < 0.3 * atom.size &&
      ((atom.x1 <= open.x0 && open.x0 - atom.x1 < 2 * open.size) || (atom.x0 >= close.x1 && atom.x0 - close.x1 < 2 * open.size)));
    const all = clusterRows(content);
    const center = all.reduce((best, row, index) =>
      Math.abs(row.base + 0.25 * row.size - axis) < Math.abs(all[best].base + 0.25 * all[best].size - axis) ? index : best, 0);
    let first = center;
    let last = center;
    while (first > 0 && all[first - 1].base - all[first].base < 1.7 * open.size && !passesThrough(all[first - 1])) first--;
    while (last < all.length - 1 && all[last].base - all[last + 1].base < 1.7 * open.size && !passesThrough(all[last + 1])) last++;
    const rows = all.slice(first, last + 1);
    if (rows.length < 2 || passesThrough(all[center]) || !rows.some((row) => Math.abs(row.base - open.base) > 0.4 * open.size)) continue;
    const members = rows.flatMap((row) => row.atoms);
    const top = Math.max(...members.map((atom) => atom.top));
    const bottom = Math.min(...members.map((atom) => atom.bottom));
    for (const atom of [open, close, ...members]) atom.consumed = true;
    const latex = GLYPH_DELIMITERS.get(open.ch);
    const environment = MATRIX_ENVIRONMENTS.get(latex) || "matrix";
    result.push(makeAtom("matrix", [open, close, ...members], {
      environment,
      open: environment === "matrix" ? latex : null,
      close: environment === "matrix" ? MATCHING_CLOSER.get(latex) : null,
      cells: splitColumns(rows, open.size),
      base: open.base,
      size: open.size,
      top,
      bottom
    }));
  }
  return result;
}

function buildComposites(glyphs, rules) {
  const atoms = glyphs;
  const created = [];
  created.push(...buildDots(atoms));
  created.push(...buildAccents(atoms));
  created.push(...processRules(atoms, rules, created));
  const live = () => atoms.concat(created).filter((atom) => !atom.consumed);
  created.push(...buildLimits(live()));
  created.push(...buildDelimiterGroups(atoms.filter((atom) => !atom.consumed), created.filter((atom) => !atom.consumed)));
  created.push(...buildGlyphMatrices(live()));
  return live();
}

// ---------------------------------------------------------------------------
// Lines

function buildLines(atoms) {
  const sorted = [...atoms].sort((a, b) => a.order - b.order);
  const lines = [];
  let line = null;
  for (const atom of sorted) {
    if (line && fitsLine(atom, line)) {
      addToLine(line, atom);
      continue;
    }
    if (atom.space) continue;
    line = { atoms: [], base: atom.base, size: atom.size, x0: atom.x0, x1: atom.x1, top: atom.top, bottom: atom.bottom, rotated: atom.rotated };
    lines.push(line);
    addToLine(line, atom);
  }
  for (const current of lines) {
    current.atoms.sort((a, b) => a.x0 - b.x0 || a.order - b.order);
    if (current.rotated) current.atoms.sort((a, b) => a.order - b.order);
    while (current.atoms.at(-1)?.space) current.atoms.pop();
    while (current.atoms[0]?.space) current.atoms.shift();
  }
  return lines.filter((current) => current.atoms.length);
}

function fitsLine(atom, line) {
  if (atom.rotated || line.rotated) return atom.rotated && line.rotated && Math.abs(atom.order - line.atoms.at(-1).order) < 2;
  const reference = line.size;
  const dy = atom.base - line.base;
  if (atom.space) return Math.abs(dy) < 0.5 * reference;
  if (Math.abs(dy) <= 0.3 * Math.max(atom.size, reference) && atom.x0 > line.x0 - 3 * reference) return true;
  if (atom.size < 0.9 * reference && Math.abs(dy) < 0.85 * reference &&
    atom.x0 >= line.x0 - 0.6 * reference && atom.x0 <= line.x1 + 1.5 * reference) return true;
  if (atom.size > 1.1 * reference && line.atoms.every((other) => other.space || other.size < 0.9 * atom.size) &&
    Math.abs(dy) < 0.85 * atom.size) {
    line.base = atom.base;
    line.size = atom.size;
    return true;
  }
  return false;
}

function addToLine(line, atom) {
  line.atoms.push(atom);
  if (atom.space) return;
  line.x0 = Math.min(line.x0, atom.x0);
  line.x1 = Math.max(line.x1, atom.x1);
  line.top = Math.max(line.top, atom.top);
  line.bottom = Math.min(line.bottom, atom.bottom);
}

// ---------------------------------------------------------------------------
// Rows: scripts and overlapping glyph pairs

export function parseRow(atoms) {
  const sorted = [...atoms].sort((a, b) => a.x0 - b.x0 || a.order - b.order);
  const nodes = [];
  for (const atom of sorted) {
    if (atom.space) {
      if (nodes.length && !nodes.at(-1).space) nodes.push({ atom, space: true });
      continue;
    }
    const base = lastNonSpace(nodes);
    if (base && isScriptOf(atom, base)) {
      (atom.base > base.atom.base ? base.sup : base.sub).push(atom);
      base.x1 = Math.max(base.x1, atom.x1);
      continue;
    }
    nodes.push({ atom, sup: [], sub: [], x1: atom.x1 });
  }
  for (const node of nodes) {
    if (node.space) continue;
    node.sup = node.sup.length ? parseRow(node.sup) : null;
    node.sub = node.sub.length ? parseRow(node.sub) : null;
  }
  while (nodes.at(-1)?.space) nodes.pop();
  return mergeEllipses(mergePairs(nodes));
}

// Key for a glyph in GLYPH_PAIRS: its LaTeX form, or a marker for TeX building blocks.
function pairKey(atom) {
  if (atom.kind === "symbol") return atom.latex;
  if (atom.kind !== "glyph" || atom.ext) return null;
  if (atom.ch === "̸" || (atom.ch === "/" && atom.font.math === "italic")) return "NOT";
  if (atom.ch === "7" && atom.font.math === "symbol") return "MAPSTO";
  if (atom.ch === "−" || atom.ch === "-") return "-";
  return MATH_SYMBOLS.get(atom.ch) ?? atom.ch;
}

function mergePairs(nodes) {
  const result = [];
  for (const node of nodes) {
    const previous = result.at(-1);
    if (!node.space && previous && !previous.space && !previous.sup && !previous.sub) {
      const left = previous.atom;
      const right = node.atom;
      const overlap = left.x1 - right.x0;
      const sameItem = left.kind === "glyph" && right.kind === "glyph" && left.item === right.item && left.item >= 0 &&
        left.offset + left.length === right.offset;
      let leftKey = pairKey(left);
      let rightKey = pairKey(right);
      if (rightKey === "NOT") [leftKey, rightKey] = [rightKey, leftKey];
      const combining = (left.ch === "\u0338" || right.ch === "\u0338") && overlap > -0.5 * right.size;
      const slash = leftKey === "NOT" && !combining;
      const negatable = rightKey !== "NOT" && (RELATIONS.has(rightKey) || GLYPH_PAIRS.has(`NOT|${rightKey}`));
      if (leftKey === "NOT" && (!negatable || (slash && overlap < 0.3 * Math.min(left.x1 - left.x0, right.x1 - right.x0)))) {
        result.push(node);
        continue;
      }
      if (leftKey && rightKey && (overlap > -0.05 * right.size || combining) && (overlap > 0.5 || sameItem || leftKey === "NOT")) {
        let latex = GLYPH_PAIRS.get(`${leftKey}|${rightKey}`);
        if (!latex && leftKey === "NOT" && rightKey !== "NOT") latex = `\\not${rightKey.startsWith("\\") ? "" : " "}${rightKey}`;
        if (latex) {
          result[result.length - 1] = {
            atom: makeAtom("symbol", [left, right], { latex, base: right.base, size: right.size }),
            sup: node.sup,
            sub: node.sub
          };
          continue;
        }
      }
    }
    result.push(node);
  }
  return result;
}

function mergeEllipses(nodes) {
  const result = [];
  for (let index = 0; index < nodes.length; index++) {
    const run = [];
    let cursor = index;
    while (cursor < nodes.length && run.length < 3) {
      const node = nodes[cursor];
      if (node.space) {
        cursor++;
        continue;
      }
      const { atom } = node;
      const isDot = atom.kind === "glyph" && atom.font.math && (atom.ch === "." || atom.ch === "·") && !node.sup && !node.sub;
      if (!isDot || (run.length && (atom.ch !== run[0].atom.ch || atom.x0 - run.at(-1).atom.x1 > 0.45 * atom.size))) break;
      run.push(node);
      cursor++;
    }
    if (run.length === 3) {
      const atoms = run.map((node) => node.atom);
      result.push({ atom: makeAtom("symbol", atoms, { latex: atoms[0].ch === "." ? "\\ldots" : "\\cdots", base: atoms[0].base, size: atoms[0].size }), sup: null, sub: null });
      index = cursor - 1;
    } else {
      result.push(nodes[index]);
    }
  }
  return result;
}

function lastNonSpace(nodes) {
  for (let index = nodes.length - 1; index >= 0; index--) {
    if (!nodes[index].space) return nodes[index];
  }
  return null;
}

function isScriptOf(atom, node) {
  const base = node.atom;
  if (atom.size >= 0.88 * base.size) return false;
  if (Math.abs(atom.base - base.base) <= 0.1 * base.size) return false;
  if (atom.x0 < base.x0 - 0.15 * base.size) return false;
  return atom.x0 <= node.x1 + 0.3 * base.size;
}

// ---------------------------------------------------------------------------
// Page model

function markSansBody(glyphs) {
  const counts = new Map();
  for (const glyph of glyphs) {
    if (glyph.space || glyph.ext || glyph.font.math || !/\p{L}/u.test(glyph.ch)) continue;
    counts.set(glyph.font, (counts.get(glyph.font) || 0) + 1);
  }
  const body = [...counts.entries()].sort((a, b) => b[1] - a[1])[0]?.[0];
  if (!body?.sans) return;
  for (const font of counts.keys()) {
    if (font.sans) font.sansBody = true;
  }
}

export function analyzePage(data) {
  const glyphs = buildGlyphs(data);
  markSansBody(glyphs);
  const context = { bodySize: estimateBodySize(glyphs) };
  const atoms = buildComposites(glyphs.filter((glyph) => !glyph.consumed), data.rules);
  const lines = buildLines(atoms);
  for (const line of lines) line.nodes = parseRow(line.atoms);
  measureMargins(lines, context, data);
  return { glyphs, lines, context, width: data.width, height: data.height };
}

// Prose lines start at the left margin; most layouts mirror it on the right.
function measureMargins(lines, context, data) {
  const prose = lines.filter((line) => !line.rotated && line.atoms[0]?.kind === "glyph" && !line.atoms[0].font.math &&
    /\p{L}/u.test(line.atoms[0].ch));
  const source = prose.length >= 2 ? prose : lines.filter((line) => !line.rotated);
  const counts = new Map();
  for (const line of source) {
    const key = Math.round(line.x0);
    counts.set(key, (counts.get(key) || 0) + 1);
  }
  const lefts = source.map((line) => line.x0).sort((a, b) => a - b);
  const common = [...counts.entries()].sort((a, b) => b[1] - a[1] || a[0] - b[0])[0]?.[0];
  context.leftMargin = common ?? (lefts.length ? lefts[0] : 0);
  const rights = source.map((line) => line.x1).sort((a, b) => a - b);
  const right = rights.length ? rights[Math.floor(rights.length * 0.9)] : data.width;
  context.rightMargin = Math.max(right, data.width - context.leftMargin);
  const gaps = [];
  for (let index = 1; index < lines.length; index++) {
    const gap = lines[index - 1].base - lines[index].base;
    if (gap > 0.8 * context.bodySize && gap < 2 * context.bodySize) gaps.push(gap);
  }
  gaps.sort((a, b) => a - b);
  context.lineSpacing = gaps.length ? gaps[Math.floor(gaps.length / 2)] : 1.2 * context.bodySize;
}
