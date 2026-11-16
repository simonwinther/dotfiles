// Classifies embedded PDF font names by their role in typeset text and math.

const EXTENSION_RE = /^(cmex|cmexb|lmex|eurm?ex|euex|txex|pxex|ntxexx?|esint|mathx|stmaryex|zxex|mtex|rmtmex)|mathextension|ex\d+$/i;
const MATH_ITALIC_RE = /^(cmmi|lmmi|eurm|eurb|txmi|txbmi|pxmi|pxbmi|ntxmi|ntxbmi|zxmi|rtxmi|rtxbmi|newtxmi)|mathitalic/i;
const MATH_SYMBOL_RE = /^(cmsy|cmbsy|lmsy|msam|msbm|txsy|txsym|pxsym|pxsy|ntxsy|eusm|eusb|eufm|eufb|rsfs|wasy|stmary|bbm|bbold|dsrom|txsya|txsyb|pxsya|pxsyb|ntxsya|ntxsyb|zxsy)|mathsymbols|^symbol(mt)?$|^symbol[-,]/i;
const UNICODE_MATH_RE = /math/i;
const BOLD_RE = /bold|black|heavy|semibold|demi|^cm(bx|b\d|bsy|mib|ssbx)|^(sf|ec|tc)(bx|bi|xc)|^cmbxti|-bd\b|medi(um)?\b|^ptmb|^phvb|^pplb|^txb|^pxb|^ntxb|bdit$/i;
const ITALIC_RE = /italic|oblique|ital\b|^cm(ti|bxti|sl|itt|ssi|u)\d|^(sf|ec|tc)(ti|sl|bi|si|it)|-it$|bdit$|^ptm(r|b)i|^lmroman.*(it|sl)/i;
const MONO_RE = /mono|courier|^cm(tt|itt|sltt)|^(sf|ec)tt|^txtt|^pcr|consol|menlo|inconsolata|typewriter|code/i;
const SANS_RE = /^cmss|^(sf|ec)ss|sans|helvetica|arial|^phv/i;

// Math style implied by a whole font, for letters set in it.
function fontMathStyle(name, math) {
  if (/^msbm|^bbm|^bbold|^dsrom|^(n?tx|px)sym$/i.test(name)) return "bb";
  if (/^eufm|^eufb/i.test(name)) return "frak";
  if (/^rsfs/i.test(name)) return "scr";
  if (/^eusm|^eusb/i.test(name)) return "cal";
  if (/^cmmib|^txbmi|^pxbmi|^ntxbmi|^rtxbmi/i.test(name)) return "bfit";
  if (/^cmbsy/i.test(name)) return "bfsym";
  if (math === "symbol" && /^(cmsy|lmsy)|mathsymbols/i.test(name)) return "calLetters";
  return null;
}

export function classifyFont(rawName) {
  const name = String(rawName || "").replace(/^[A-Z]{6}\+/, "");
  let math = null;
  if (EXTENSION_RE.test(name)) math = "extension";
  else if (MATH_ITALIC_RE.test(name)) math = "italic";
  else if (MATH_SYMBOL_RE.test(name)) math = "symbol";
  else if (UNICODE_MATH_RE.test(name) && !/mathematica/i.test(name)) math = "unicode";
  return {
    name,
    math,
    style: math ? fontMathStyle(name, math) : null,
    bold: BOLD_RE.test(name),
    italic: ITALIC_RE.test(name),
    mono: MONO_RE.test(name),
    sans: SANS_RE.test(name)
  };
}
