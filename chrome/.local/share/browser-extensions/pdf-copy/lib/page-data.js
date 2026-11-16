// Reads the positioned text, math extension glyphs, and thin rules of a PDF page.
import { classifyFont } from "./fonts.js";

const IDENTITY = [1, 0, 0, 1, 0, 0];

function multiply(m, n) {
  return [
    m[0] * n[0] + m[2] * n[1],
    m[1] * n[0] + m[3] * n[1],
    m[0] * n[2] + m[2] * n[3],
    m[1] * n[2] + m[3] * n[3],
    m[0] * n[4] + m[2] * n[5] + m[4],
    m[1] * n[4] + m[3] * n[5] + m[5]
  ];
}

function apply(m, x, y) {
  return [m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]];
}

export async function readPageData(page, OPS) {
  // The operator list also loads the page fonts into commonObjs.
  const operatorList = await page.getOperatorList();
  // Same options as the PDF.js text layer, so item indices match its spans.
  const textContent = await page.getTextContent({ includeMarkedContent: true, disableNormalization: true });
  const [left, bottom, right, top] = page.view;
  const fonts = new Map();

  function fontInfo(loadedName) {
    if (!fonts.has(loadedName)) {
      let font = null;
      try {
        font = page.commonObjs.has(loadedName) ? page.commonObjs.get(loadedName) : null;
      } catch {
        font = null;
      }
      fonts.set(loadedName, {
        ...classifyFont(font?.name || ""),
        fontMatrix: font?.fontMatrix || [0.001, 0, 0, 0.001, 0, 0],
        type3: Boolean(font?.isType3Font)
      });
    }
    return fonts.get(loadedName);
  }

  const items = [];
  for (const item of textContent.items) {
    if (item.str === undefined) continue;
    const [a, b, c, d, e, f] = item.transform;
    items.push({
      str: item.str,
      font: fontInfo(item.fontName),
      size: Math.hypot(c, d) || Math.hypot(a, b),
      x: e - left,
      y: f - bottom,
      width: item.width,
      horizontal: Math.abs(b) < 1e-3 * Math.abs(a) && Math.abs(c) < 1e-3 * Math.abs(d) && a > 0 && d > 0 && !item.vertical
    });
  }

  const { extensionGlyphs, rules } = scanOperators(operatorList, OPS, fontInfo);
  for (const glyph of extensionGlyphs) {
    glyph.x -= left;
    glyph.y -= bottom;
  }
  for (const rule of rules) {
    rule.x0 -= left;
    rule.x1 -= left;
    rule.y0 -= bottom;
    rule.y1 -= bottom;
  }
  return { width: right - left, height: top - bottom, items, extensionGlyphs, rules };
}

// Follows the text and graphics state to place glyphs that the text layer drops.
function scanOperators({ fnArray, argsArray }, OPS, fontInfo) {
  const extensionGlyphs = [];
  const rules = [];
  const stack = [];
  let state = {
    ctm: IDENTITY,
    lineWidth: 1,
    font: null,
    fontSize: 0,
    charSpacing: 0,
    wordSpacing: 0,
    hScale: 1,
    leading: 0,
    rise: 0,
    textMatrix: IDENTITY,
    x: 0,
    y: 0,
    lineX: 0,
    lineY: 0
  };

  function moveText(dx, dy) {
    state.lineX += dx;
    state.lineY += dy;
    state.x = state.lineX;
    state.y = state.lineY;
  }

  function addRule(op, minMax) {
    if (!minMax) return;
    const isStroke = op === OPS.stroke || op === OPS.closeStroke;
    const isFill = op === OPS.fill || op === OPS.eoFill || op === OPS.fillStroke ||
      op === OPS.eoFillStroke || op === OPS.closeFillStroke || op === OPS.closeEOFillStroke;
    if (!isStroke && !isFill) return;
    const [x0, y0, x1, y1] = minMax;
    const corners = [apply(state.ctm, x0, y0), apply(state.ctm, x1, y0), apply(state.ctm, x0, y1), apply(state.ctm, x1, y1)];
    const xs = corners.map((point) => point[0]);
    const ys = corners.map((point) => point[1]);
    const scale = Math.sqrt(Math.abs(state.ctm[0] * state.ctm[3] - state.ctm[1] * state.ctm[2]));
    const half = isStroke ? (state.lineWidth * scale) / 2 : 0;
    const rule = {
      x0: Math.min(...xs) - (isStroke && x0 === x1 ? half : 0),
      x1: Math.max(...xs) + (isStroke && x0 === x1 ? half : 0),
      y0: Math.min(...ys) - (isStroke && y0 === y1 ? half : 0),
      y1: Math.max(...ys) + (isStroke && y0 === y1 ? half : 0)
    };
    const width = rule.x1 - rule.x0;
    const height = rule.y1 - rule.y0;
    if ((height <= 2 && width >= 1) || (width <= 2 && height >= 4)) rules.push(rule);
  }

  function showText(glyphs) {
    const { font, fontSize } = state;
    if (!font || fontSize === 0) return;
    const direction = fontSize < 0 ? -1 : 1;
    const size = Math.abs(fontSize);
    const advanceScale = size * (font.fontMatrix[0] || 0.001);
    const matrix = multiply(state.ctm, state.textMatrix);
    let x = 0;
    for (const glyph of glyphs) {
      if (typeof glyph === "number") {
        x -= (glyph * size) / 1000;
        continue;
      }
      if (!glyph) continue;
      const spacing = state.charSpacing + (glyph.isSpace ? state.wordSpacing : 0);
      const advance = glyph.width * advanceScale + spacing * direction;
      if (font.math === "extension") {
        const start = apply(matrix, state.x + x * state.hScale, state.y + state.rise);
        const end = apply(matrix, state.x + (x + glyph.width * advanceScale) * state.hScale, state.y + state.rise);
        const scale = Math.hypot(matrix[2], matrix[3]) * size;
        extensionGlyphs.push({
          code: glyph.originalCharCode,
          unicode: glyph.unicode || "",
          font,
          size: scale,
          x: start[0],
          y: start[1],
          width: Math.max(0, end[0] - start[0])
        });
      }
      x += advance;
    }
    state.x += x * state.hScale;
  }

  for (let index = 0; index < fnArray.length; index++) {
    const fn = fnArray[index];
    const args = argsArray[index] || [];
    switch (fn) {
      case OPS.save:
        stack.push({ ...state });
        break;
      case OPS.restore:
        state = stack.pop() || state;
        break;
      case OPS.transform:
        state.ctm = multiply(state.ctm, args);
        break;
      case OPS.paintFormXObjectBegin:
        stack.push({ ...state });
        if (Array.isArray(args[0]) || ArrayBuffer.isView(args[0])) state.ctm = multiply(state.ctm, Array.from(args[0]));
        break;
      case OPS.paintFormXObjectEnd:
        state = stack.pop() || state;
        break;
      case OPS.setLineWidth:
        state.lineWidth = args[0];
        break;
      case OPS.beginText:
        state.textMatrix = IDENTITY;
        state.x = state.y = state.lineX = state.lineY = 0;
        break;
      case OPS.setTextMatrix:
        state.textMatrix = Array.from(args.length === 1 ? args[0] : args);
        state.x = state.y = state.lineX = state.lineY = 0;
        break;
      case OPS.moveText:
        moveText(args[0], args[1]);
        break;
      case OPS.setLeadingMoveText:
        state.leading = args[1];
        moveText(args[0], args[1]);
        break;
      case OPS.nextLine:
        moveText(0, state.leading);
        break;
      case OPS.setLeading:
        state.leading = -args[0];
        break;
      case OPS.setFont:
        state.font = fontInfo(args[0]);
        state.fontSize = args[1];
        break;
      case OPS.setCharSpacing:
        state.charSpacing = args[0];
        break;
      case OPS.setWordSpacing:
        state.wordSpacing = args[0];
        break;
      case OPS.setHScale:
        state.hScale = args[0] / 100;
        break;
      case OPS.setTextRise:
        state.rise = args[0];
        break;
      case OPS.showText:
        showText(args[0]);
        break;
      case OPS.constructPath:
        addRule(args[0], args[2]);
        break;
      default:
        break;
    }
  }
  return { extensionGlyphs, rules };
}
