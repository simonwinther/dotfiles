// Symbol tables for turning PDF glyphs back into LaTeX source.

export const MATH_SYMBOLS = new Map(Object.entries({
  // Greek
  "α": "\\alpha", "β": "\\beta", "γ": "\\gamma", "δ": "\\delta", "ϵ": "\\epsilon",
  "ε": "\\varepsilon", "ζ": "\\zeta", "η": "\\eta", "θ": "\\theta", "ϑ": "\\vartheta",
  "ι": "\\iota", "κ": "\\kappa", "λ": "\\lambda", "μ": "\\mu", "µ": "\\mu", "ν": "\\nu",
  "ξ": "\\xi", "π": "\\pi", "ϖ": "\\varpi", "ρ": "\\rho", "ϱ": "\\varrho", "σ": "\\sigma",
  "ς": "\\varsigma", "τ": "\\tau", "υ": "\\upsilon", "ϕ": "\\phi", "φ": "\\varphi",
  "χ": "\\chi", "ψ": "\\psi", "ω": "\\omega", "ϰ": "\\varkappa",
  "Γ": "\\Gamma", "Δ": "\\Delta", "∆": "\\Delta", "Θ": "\\Theta", "Λ": "\\Lambda",
  "Ξ": "\\Xi", "Π": "\\Pi", "Σ": "\\Sigma", "Υ": "\\Upsilon", "ϒ": "\\Upsilon",
  "\u2126": "\\Omega", "Φ": "\\Phi", "Ψ": "\\Psi", "Ω": "\\Omega", "Ω": "\\Omega",
  // Binary operators
  "−": "-", "–": "-", "∗": "\\ast", "±": "\\pm", "∓": "\\mp", "×": "\\times",
  "÷": "\\div", "·": "\\cdot", "⋅": "\\cdot", "∘": "\\circ", "◦": "\\circ",
  "•": "\\bullet", "∙": "\\bullet", "⋆": "\\star", "∩": "\\cap", "∪": "\\cup",
  "⊎": "\\uplus", "⊓": "\\sqcap", "⊔": "\\sqcup", "∨": "\\vee", "∧": "\\wedge",
  "⊕": "\\oplus", "⊖": "\\ominus", "⊗": "\\otimes", "⊘": "\\oslash", "⊙": "\\odot",
  "∖": "\\setminus", "≀": "\\wr", "⋄": "\\diamond", "△": "\\bigtriangleup",
  "▽": "\\bigtriangledown", "◁": "\\triangleleft", "▷": "\\triangleright",
  "†": "\\dagger", "‡": "\\ddagger", "⨿": "\\amalg", "⊞": "\\boxplus",
  "⊠": "\\boxtimes", "⊡": "\\boxdot", "⊟": "\\boxminus", "⋉": "\\ltimes", "⋊": "\\rtimes",
  // Relations
  "≠": "\\neq", "≤": "\\leq", "≥": "\\geq", "⩽": "\\leqslant", "⩾": "\\geqslant",
  "≪": "\\ll", "≫": "\\gg", "≈": "\\approx", "≅": "\\cong", "≡": "\\equiv",
  "∼": "\\sim", "≃": "\\simeq", "∝": "\\propto", "≺": "\\prec", "≻": "\\succ",
  "⪯": "\\preceq", "⪰": "\\succeq", "⊂": "\\subset", "⊃": "\\supset",
  "⊆": "\\subseteq", "⊇": "\\supseteq", "⊊": "\\subsetneq", "⊋": "\\supsetneq",
  "⊈": "\\nsubseteq", "⊉": "\\nsupseteq", "⊏": "\\sqsubset", "⊐": "\\sqsupset",
  "⊑": "\\sqsubseteq", "⊒": "\\sqsupseteq", "∈": "\\in", "∉": "\\notin", "∋": "\\ni",
  "⊢": "\\vdash", "⊣": "\\dashv", "⊨": "\\models", "∣": "\\mid", "∥": "\\parallel",
  "⊥": "\\perp", "⌣": "\\smile", "⌢": "\\frown", "≍": "\\asymp", "≐": "\\doteq",
  "⋈": "\\bowtie", "≲": "\\lesssim", "≳": "\\gtrsim", "≜": "\\triangleq",
  "≔": "\\coloneqq", "∴": "\\therefore", "∵": "\\because", "≮": "\\nless",
  "≯": "\\ngtr", "≰": "\\nleq", "≱": "\\ngeq", "≁": "\\nsim", "≇": "\\ncong",
  "∦": "\\nparallel", "⊲": "\\lhd", "⊳": "\\rhd", "⊴": "\\unlhd", "⊵": "\\unrhd",
  // Arrows
  "→": "\\to", "←": "\\leftarrow", "↔": "\\leftrightarrow", "⇒": "\\Rightarrow",
  "⇐": "\\Leftarrow", "⇔": "\\Leftrightarrow", "↦": "\\mapsto", "↑": "\\uparrow",
  "↓": "\\downarrow", "↕": "\\updownarrow", "⇑": "\\Uparrow", "⇓": "\\Downarrow",
  "⟶": "\\longrightarrow", "⟵": "\\longleftarrow", "⟷": "\\longleftrightarrow",
  "⟹": "\\Longrightarrow", "⟸": "\\Longleftarrow", "⟺": "\\Longleftrightarrow",
  "⟼": "\\longmapsto", "↪": "\\hookrightarrow", "↩": "\\hookleftarrow",
  "⇀": "\\rightharpoonup", "↼": "\\leftharpoonup", "⇌": "\\rightleftharpoons",
  "↗": "\\nearrow", "↘": "\\searrow", "↙": "\\swarrow", "↖": "\\nwarrow",
  "⇝": "\\leadsto", "↠": "\\twoheadrightarrow",
  // Miscellaneous
  "∞": "\\infty", "∂": "\\partial", "∇": "\\nabla", "∀": "\\forall", "∃": "\\exists",
  "∄": "\\nexists", "¬": "\\neg", "∅": "\\emptyset", "ℵ": "\\aleph", "ℏ": "\\hbar",
  "ℓ": "\\ell", "℘": "\\wp", "ℜ": "\\Re", "ℑ": "\\Im", "′": "\\prime", "″": "\\prime\\prime", "‴": "\\prime\\prime\\prime",
  "…": "\\ldots", "⋯": "\\cdots", "⋮": "\\vdots", "⋱": "\\ddots", "∠": "\\angle",
  "⊤": "\\top", "♭": "\\flat", "♮": "\\natural", "♯": "\\sharp", "♣": "\\clubsuit",
  "♢": "\\diamondsuit", "♡": "\\heartsuit", "♠": "\\spadesuit", "√": "\\surd",
  "ı": "\\imath", "ȷ": "\\jmath", "□": "\\square", "■": "\\blacksquare",
  "◇": "\\Diamond", "★": "\\bigstar", "○": "\\bigcirc", "◯": "\\bigcirc",
  "∡": "\\measuredangle", "ð": "\\eth", "ℶ": "\\beth", "ℷ": "\\gimel",
  "ℝ": "\\mathbb{R}", "ℕ": "\\mathbb{N}", "ℤ": "\\mathbb{Z}", "ℚ": "\\mathbb{Q}",
  "ℂ": "\\mathbb{C}", "ℙ": "\\mathbb{P}", "ℍ": "\\mathbb{H}", "ℰ": "\\mathcal{E}",
  "ℱ": "\\mathcal{F}", "ℋ": "\\mathcal{H}", "ℐ": "\\mathcal{I}", "ℒ": "\\mathcal{L}",
  "ℳ": "\\mathcal{M}", "ℛ": "\\mathcal{R}", "ℬ": "\\mathcal{B}", "ℯ": "e",
  "ℊ": "g", "ℴ": "o", "ℎ": "h", "ℭ": "\\mathfrak{C}", "ℌ": "\\mathfrak{H}",
  "ℨ": "\\mathfrak{Z}", "°": "^{\\circ}",
  // Big operators
  "∑": "\\sum", "∏": "\\prod", "∐": "\\coprod", "∫": "\\int", "∬": "\\iint",
  "∭": "\\iiint", "∮": "\\oint", "⋂": "\\bigcap", "⋃": "\\bigcup", "⨀": "\\bigodot",
  "⨁": "\\bigoplus", "⨂": "\\bigotimes", "⋁": "\\bigvee", "⋀": "\\bigwedge",
  "⨄": "\\biguplus", "⨆": "\\bigsqcup",
  // Delimiters
  "⟨": "\\langle", "⟩": "\\rangle", "〈": "\\langle", "〉": "\\rangle",
  "⌈": "\\lceil", "⌉": "\\rceil", "⌊": "\\lfloor", "⌋": "\\rfloor", "{": "\\{",
  "}": "\\}", "‖": "\\|", "\\": "\\setminus", "|": "|",
  // Characters that need escaping in math
  "#": "\\#", "$": "\\$", "%": "\\%", "&": "\\&", "_": "\\_", "~": "\\sim",
  "∕": "/", "⁄": "/", "‐": "-", "ʹ": "'", "∶": ":", "∣": "\\mid"
}));

export const BIG_OPERATORS = new Set([
  "\\sum", "\\prod", "\\coprod", "\\int", "\\iint", "\\iiint", "\\oint", "\\bigcap",
  "\\bigcup", "\\bigodot", "\\bigoplus", "\\bigotimes", "\\bigvee", "\\bigwedge",
  "\\biguplus", "\\bigsqcup"
]);

export const RELATIONS = new Set([
  "=", "<", ">", ":=", "\\neq", "\\leq", "\\geq", "\\leqslant", "\\geqslant", "\\ll",
  "\\gg", "\\approx", "\\cong", "\\equiv", "\\sim", "\\simeq", "\\propto", "\\prec",
  "\\succ", "\\preceq", "\\succeq", "\\subset", "\\supset", "\\subseteq", "\\supseteq",
  "\\subsetneq", "\\supsetneq", "\\nsubseteq", "\\nsupseteq", "\\sqsubset", "\\sqsupset",
  "\\sqsubseteq", "\\sqsupseteq", "\\in", "\\notin", "\\ni", "\\vdash", "\\dashv",
  "\\models", "\\mid", "\\parallel", "\\perp", "\\smile", "\\frown", "\\asymp",
  "\\doteq", "\\bowtie", "\\lesssim", "\\gtrsim", "\\triangleq", "\\coloneqq",
  "\\nless", "\\ngtr", "\\nleq", "\\ngeq", "\\nsim", "\\ncong", "\\nparallel", "\\to",
  "\\leftarrow", "\\leftrightarrow", "\\Rightarrow", "\\Leftarrow", "\\Leftrightarrow",
  "\\mapsto", "\\uparrow", "\\downarrow", "\\longrightarrow", "\\longleftarrow",
  "\\longleftrightarrow", "\\Longrightarrow", "\\Longleftarrow", "\\Longleftrightarrow",
  "\\longmapsto", "\\hookrightarrow", "\\hookleftarrow", "\\rightleftharpoons",
  "\\leadsto", "\\twoheadrightarrow", "\\therefore", "\\because", "\\lhd", "\\rhd",
  "\\unlhd", "\\unrhd", "\\nearrow", "\\searrow", "\\swarrow", "\\nwarrow"
]);

export const BINARY_OPERATORS = new Set([
  "+", "-", "\\pm", "\\mp", "\\times", "\\div", "\\cdot", "\\circ", "\\ast", "\\bullet",
  "\\star", "\\cap", "\\cup", "\\uplus", "\\sqcap", "\\sqcup", "\\vee", "\\wedge",
  "\\oplus", "\\ominus", "\\otimes", "\\oslash", "\\odot", "\\setminus", "\\wr",
  "\\diamond", "\\dagger", "\\ddagger", "\\amalg", "\\boxplus", "\\boxtimes", "\\boxdot",
  "\\boxminus", "\\ltimes", "\\rtimes", "*"
]);

export const OPENING_DELIMITERS = new Set(["(", "[", "\\{", "\\langle", "\\lceil", "\\lfloor"]);
export const CLOSING_DELIMITERS = new Set([")", "]", "\\}", "\\rangle", "\\rceil", "\\rfloor"]);

export const OPERATOR_NAMES = new Set([
  "sin", "cos", "tan", "cot", "sec", "csc", "arcsin", "arccos", "arctan", "sinh", "cosh",
  "tanh", "coth", "exp", "log", "ln", "lg", "lim", "liminf", "limsup", "max", "min", "sup",
  "inf", "det", "dim", "ker", "deg", "gcd", "hom", "arg", "Pr", "mod", "bmod"
]);

// Common names that amsmath does not predefine.
export const EXTRA_OPERATOR_NAMES = new Set([
  "tr", "Tr", "trace", "rank", "diag", "sgn", "sign", "argmax", "argmin", "Var", "Cov",
  "span", "Re", "Im", "softmax", "erf", "lcm", "proj", "vec", "id", "card", "supp",
  "dist", "dom", "im", "range", "null", "Ker"
]);

export const ACCENTS = new Map(Object.entries({
  "ˆ": "\\hat", "^": "\\hat", "̂": "\\hat", "ˇ": "\\check", "̌": "\\check",
  "˜": "\\tilde", "~": "\\tilde", "̃": "\\tilde", "´": "\\acute", "́": "\\acute",
  "`": "\\grave", "̀": "\\grave", "˙": "\\dot", "̇": "\\dot", "¨": "\\ddot",
  "̈": "\\ddot", "˘": "\\breve", "̆": "\\breve", "¯": "\\bar", "̄": "\\bar",
  "ˉ": "\\bar", "⃗": "\\vec", "→": "\\vec", "˚": "\\mathring", "̊": "\\mathring"
}));

export const TEXT_ACCENTS = new Map(Object.entries({
  "ˆ": "̂", "^": "̂", "ˇ": "̌", "˜": "̃", "~": "̃",
  "´": "́", "`": "̀", "˙": "̇", "¨": "̈", "˘": "̆",
  "¯": "̄", "˚": "̊", "¸": "̧", "˛": "̨", "˝": "̋"
}));

// Arrows and relations that TeX builds from two overlapping glyphs.
export const GLYPH_PAIRS = new Map(Object.entries({
  "-|\\to": "\\longrightarrow",
  "\\leftarrow|-": "\\longleftarrow",
  "=|\\Rightarrow": "\\Longrightarrow",
  "\\Leftarrow|=": "\\Longleftarrow",
  "\\Leftarrow|\\Rightarrow": "\\Longleftrightarrow",
  "\\leftarrow|\\to": "\\longleftrightarrow",
  "MAPSTO|\\to": "\\mapsto",
  "MAPSTO|\\longrightarrow": "\\longmapsto",
  "\\sim|=": "\\cong",
  "=|\\sim": "\\cong",
  "NOT|=": "\\neq",
  "NOT|\\in": "\\notin",
  ":|=": "\\coloneqq",
  "\\hookleftarrow|\\to": "\\hookrightarrow"
}));

// Mathematical Alphanumeric Symbols (U+1D400..U+1D7FF), by block offset.
const ALPHANUMERIC_STYLES = [
  ["bf", 0x1d400], ["it", 0x1d434], ["bfit", 0x1d468], ["cal", 0x1d49c],
  ["bfcal", 0x1d4d0], ["frak", 0x1d504], ["bb", 0x1d538], ["bffrak", 0x1d56c],
  ["sf", 0x1d5a0], ["bfsf", 0x1d5d4], ["sfit", 0x1d608], ["bfsfit", 0x1d63c],
  ["tt", 0x1d670]
];
const GREEK_STYLES = [["bf", 0x1d6a8], ["it", 0x1d6e2], ["bfit", 0x1d71c], ["bfsf", 0x1d756], ["bfsfit", 0x1d790]];
const GREEK_ORDER = "ΑΒΓΔΕΖΗΘΙΚΛΜΝΞΟΠΡϴΣΤΥΦΧΨΩ∇αβγδεζηθικλμνξοπρςστυφχψω∂ϵϑϰϕϱϖ";
const DIGIT_STYLES = [["bf", 0x1d7ce], ["bb", 0x1d7d8], ["sf", 0x1d7e2], ["bfsf", 0x1d7ec], ["tt", 0x1d7f6]];
const LETTERS = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

// Returns { char, style } for a styled mathematical alphanumeric, or null.
export function decodeMathAlphanumeric(char) {
  const code = char.codePointAt(0);
  if (code < 0x1d400 || code > 0x1d7ff) return null;
  if (code === 0x1d6a4) return { char: "ı", style: "it" };
  if (code === 0x1d6a5) return { char: "ȷ", style: "it" };
  for (let index = DIGIT_STYLES.length - 1; index >= 0; index--) {
    const [style, start] = DIGIT_STYLES[index];
    if (code >= start && code < start + 10) return { char: String(code - start), style };
  }
  for (let index = GREEK_STYLES.length - 1; index >= 0; index--) {
    const [style, start] = GREEK_STYLES[index];
    if (code >= start && code < start + GREEK_ORDER.length) {
      return { char: GREEK_ORDER[code - start], style };
    }
  }
  for (let index = ALPHANUMERIC_STYLES.length - 1; index >= 0; index--) {
    const [style, start] = ALPHANUMERIC_STYLES[index];
    if (code >= start && code < start + 52) return { char: LETTERS[code - start], style };
  }
  return null;
}

// Font style commands for letters in math.
export const STYLE_COMMANDS = {
  bf: "\\mathbf", bfit: "\\boldsymbol", cal: "\\mathcal", bfcal: "\\boldsymbol{\\mathcal",
  frak: "\\mathfrak", bffrak: "\\mathfrak", bb: "\\mathbb", sf: "\\mathsf",
  bfsf: "\\mathsf", sfit: "\\mathsf", bfsfit: "\\mathsf", tt: "\\mathtt", rm: "\\mathrm",
  scr: "\\mathscr", bfsym: "\\boldsymbol"
};

// Computer Modern math extension (cmex10) glyphs, indexed by character code.
// Sizes are the glyph's total height in em; glyphs hang below their baseline.
const CMEX_DELIMITERS = [
  ["(", ")", "[", "]", "\\lfloor", "\\rfloor", "\\lceil", "\\rceil", "\\{", "\\}", "\\langle", "\\rangle"],
  ["(", ")", "[", "]", "\\lfloor", "\\rfloor", "\\lceil", "\\rceil", "\\{", "\\}", "\\langle", "\\rangle", "/", "\\backslash"]
];

function delimiter(latex, size) {
  return { kind: "delimiter", latex, size, side: delimiterSide(latex) };
}

function delimiterSide(latex) {
  if (OPENING_DELIMITERS.has(latex)) return "left";
  if (CLOSING_DELIMITERS.has(latex)) return "right";
  return "both";
}

export const CMEX = new Map();
CMEX_DELIMITERS[0].forEach((latex, index) => CMEX.set(index, delimiter(latex, 1.2)));
CMEX.set(0x0c, { kind: "piece", latex: "|", part: "ex", size: 0.6 });
CMEX.set(0x0d, { kind: "piece", latex: "\\|", part: "ex", size: 0.6 });
CMEX.set(0x0e, delimiter("/", 1.2));
CMEX.set(0x0f, delimiter("\\backslash", 1.2));
CMEX.set(0x10, delimiter("(", 1.8));
CMEX.set(0x11, delimiter(")", 1.8));
CMEX_DELIMITERS[1].forEach((latex, index) => CMEX.set(0x12 + index, delimiter(latex, 2.4)));
CMEX_DELIMITERS[1].forEach((latex, index) => CMEX.set(0x20 + index, delimiter(latex, 3.0)));
CMEX.set(0x2e, delimiter("/", 1.8));
CMEX.set(0x2f, delimiter("\\backslash", 1.8));
for (const [code, latex, part, size] of [
  [0x30, "(", "top", 1.8], [0x31, ")", "top", 1.8], [0x32, "[", "top", 1.8],
  [0x33, "]", "top", 1.8], [0x34, "[", "bottom", 1.8], [0x35, "]", "bottom", 1.8],
  [0x36, "[", "ex", 0.6], [0x37, "]", "ex", 0.6], [0x38, "\\{", "top", 0.9],
  [0x39, "\\}", "top", 0.9], [0x3a, "\\{", "bottom", 0.9], [0x3b, "\\}", "bottom", 0.9],
  [0x3c, "\\{", "middle", 1.8], [0x3d, "\\}", "middle", 1.8], [0x3e, "\\{", "ex", 0.3],
  [0x3f, "|", "ex", 0.6], [0x40, "(", "bottom", 1.8], [0x41, ")", "bottom", 1.8],
  [0x42, "(", "ex", 0.6], [0x43, ")", "ex", 0.6]
]) {
  CMEX.set(code, { kind: "piece", latex, part, size });
}
CMEX.set(0x44, delimiter("\\langle", 1.8));
CMEX.set(0x45, delimiter("\\rangle", 1.8));
for (const [code, latex] of [
  [0x46, "\\bigsqcup"], [0x48, "\\oint"], [0x4a, "\\bigodot"], [0x4c, "\\bigoplus"],
  [0x4e, "\\bigotimes"], [0x50, "\\sum"], [0x51, "\\prod"], [0x52, "\\int"],
  [0x53, "\\bigcup"], [0x54, "\\bigcap"], [0x55, "\\biguplus"], [0x56, "\\bigwedge"],
  [0x57, "\\bigvee"], [0x60, "\\coprod"]
]) {
  CMEX.set(code, { kind: "operator", latex, size: latex === "\\int" || latex === "\\oint" ? 1.11 : 1.0 });
}
for (const [code, latex] of [
  [0x47, "\\bigsqcup"], [0x49, "\\oint"], [0x4b, "\\bigodot"], [0x4d, "\\bigoplus"],
  [0x4f, "\\bigotimes"], [0x58, "\\sum"], [0x59, "\\prod"], [0x5a, "\\int"],
  [0x5b, "\\bigcup"], [0x5c, "\\bigcap"], [0x5d, "\\biguplus"], [0x5e, "\\bigwedge"],
  [0x5f, "\\bigvee"], [0x61, "\\coprod"]
]) {
  CMEX.set(code, { kind: "operator", latex, size: latex === "\\int" || latex === "\\oint" ? 2.22 : 1.4 });
}
for (const code of [0x62, 0x63, 0x64]) CMEX.set(code, { kind: "accent", latex: "\\widehat", size: 0.3 });
for (const code of [0x65, 0x66, 0x67]) CMEX.set(code, { kind: "accent", latex: "\\widetilde", size: 0.3 });
CMEX.set(0x68, delimiter("[", 1.8));
CMEX.set(0x69, delimiter("]", 1.8));
CMEX.set(0x6a, delimiter("\\lfloor", 1.8));
CMEX.set(0x6b, delimiter("\\rfloor", 1.8));
CMEX.set(0x6c, delimiter("\\lceil", 1.8));
CMEX.set(0x6d, delimiter("\\rceil", 1.8));
CMEX.set(0x6e, delimiter("\\{", 1.8));
CMEX.set(0x6f, delimiter("\\}", 1.8));
CMEX.set(0x70, { kind: "radical", size: 1.2 });
CMEX.set(0x71, { kind: "radical", size: 1.8 });
CMEX.set(0x72, { kind: "radical", size: 2.4 });
CMEX.set(0x73, { kind: "radical", size: 3.0 });
CMEX.set(0x74, { kind: "radical", part: "bottom", size: 1.8 });
CMEX.set(0x75, { kind: "radical", part: "ex", size: 0.6 });
CMEX.set(0x76, { kind: "radical", part: "top", size: 0.6 });
CMEX.set(0x77, { kind: "piece", latex: "\\|", part: "ex", size: 0.6 });

// Type 1 versions of the TeX fonts repeat the control-character codes higher up.
export function cmexEntry(code) {
  if (code >= 0xa1 && code <= 0xaa) return CMEX.get(code - 0xa1);
  if (code >= 0xad && code <= 0xc3) return CMEX.get(code - 0xa3);
  if (code === 0xc4) return CMEX.get(0x7f);
  return CMEX.get(code);
}

// Unicode extensible delimiter pieces, used by OpenType math fonts.
export const UNICODE_PIECES = new Map(Object.entries({
  "⎛": ["(", "top"], "⎜": ["(", "ex"], "⎝": ["(", "bottom"],
  "⎞": [")", "top"], "⎟": [")", "ex"], "⎠": [")", "bottom"],
  "⎡": ["[", "top"], "⎢": ["[", "ex"], "⎣": ["[", "bottom"],
  "⎤": ["]", "top"], "⎥": ["]", "ex"], "⎦": ["]", "bottom"],
  "⎧": ["\\{", "top"], "⎨": ["\\{", "middle"], "⎩": ["\\{", "bottom"], "⎪": ["\\{", "ex"],
  "⎫": ["\\}", "top"], "⎬": ["\\}", "middle"], "⎭": ["\\}", "bottom"],
  "⎮": ["\\int", "ex"], "⌠": ["\\int", "top"], "⌡": ["\\int", "bottom"],
  "⏐": ["|", "ex"], "⎸": ["|", "ex"], "⎹": ["|", "ex"],
  // Adobe private-use names for the same pieces.
  "": ["(", "top"], "": ["(", "ex"], "": ["(", "bottom"],
  "": [")", "top"], "": [")", "ex"], "": [")", "bottom"],
  "": ["[", "top"], "": ["[", "ex"], "": ["[", "bottom"],
  "": ["]", "top"], "": ["]", "ex"], "": ["]", "bottom"],
  "": ["\\{", "top"], "": ["\\{", "middle"], "": ["\\{", "bottom"],
  "": ["\\{", "ex"], "": ["\\}", "top"], "": ["\\}", "middle"],
  "": ["\\}", "bottom"], "": ["|", "ex"]
}));

// TeX font encodings for the control-character codes that PDF.js passes through
// when an embedded font has no glyph names.
const OML_LOW = "ΓΔΘΛΞΠΣΥΦΨΩαβγδϵζηθικλμνξπρστυϕχψωεϑϖϱςφ";
const OMS_LOW = "−·×∗÷⋄±∓⊕⊖⊗⊘⊙○∘•≍≡⊆⊇≤≥⪯⪰∼≈⊂⊃≪≫≺≻";
const OT1_LOW = ["Γ", "Δ", "Θ", "Λ", "Ξ", "Π", "Σ", "Υ", "Φ", "Ψ", "Ω", "ff", "fi", "fl", "ffi", "ffl", "ı", "ȷ", "`", "´", "ˇ", "˘", "¯", "˚", "¸", "ß", "æ", "œ", "ø", "Æ", "Œ", "Ø"];

export function controlCharacter(char, font) {
  const code = char.codePointAt(0);
  if (code > 0x27) return null;
  if (font.math === "italic") return Array.from(OML_LOW)[code] ?? null;
  if (font.math === "symbol") return code < 0x20 ? Array.from(OMS_LOW)[code] : null;
  if (!font.math && code < 0x20) return OT1_LOW[code];
  return null;
}

export const MATRIX_ENVIRONMENTS = new Map(Object.entries({
  "(": "pmatrix", "[": "bmatrix", "\\{": "Bmatrix", "|": "vmatrix", "\\|": "Vmatrix"
}));
