import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import vm from "node:vm";
import temml from "./vendor/temml/temml.js";

const context = vm.createContext({ window: {} });
vm.runInContext(readFileSync(new URL("./math.js", import.meta.url), "utf8"), context);
const split = (text) => JSON.parse(JSON.stringify(context.window.__chatgptPdfMath.split(text)));
const equations = (text) => split(text).filter((part) => part.type === "math");

test("recognizes all four common math delimiters", () => {
  const text = String.raw`Before $x_t$ then \(s\), $$\frac{1}{2}$$ and \[\epsilon\] after.`;
  assert.deepEqual(equations(text).map(({ expression, display }) => ({ expression, display })), [
    { expression: "x_t", display: false }, { expression: "s", display: false },
    { expression: String.raw`\frac{1}{2}`, display: true }, { expression: String.raw`\epsilon`, display: true }
  ]);
  assert.equal(split(text).map((part) => part.raw).join(""), text);
});

test("keeps multiline expressions intact across blank lines", () => {
  const text = "Before\n\n$$\nx_t = \\frac{1}{2}\n\n+ \\sqrt{t(1-t)}\\epsilon\n$$\n\nAfter";
  assert.equal(equations(text).length, 1);
  assert.match(equations(text)[0].expression, /\n\n/);
  assert.equal(split(text).map((part) => part.raw).join(""), text);
});

test("skips inline code and both kinds of fenced code", () => {
  const text = "`$x$` and ``\\(y\\)``\n```tex\n$$z$$\n```\n~~~tex\n\\[w\\]\n~~~\nActual $a$";
  assert.deepEqual(equations(text).map((part) => part.expression), ["a"]);
});

test("leaves escaped delimiters, currency, and unmatched delimiters unchanged", () => {
  for (const text of [String.raw`Escaped \$x\$ and \\(y\\)`, "Costs $5 and $10.", "Costs $5–$10.", "Only $x", "Only $$x", String.raw`Only \[x`]) {
    assert.equal(equations(text).length, 0, text);
    assert.equal(split(text).map((part) => part.raw).join(""), text);
  }
});

test("matches dollars inside braces and escaped closing characters correctly", () => {
  const text = String.raw`$\text{rate $/s} + x$ and $\text{cost \$} + y$`;
  assert.deepEqual(equations(text).map((part) => part.expression), [String.raw`\text{rate $/s} + x`, String.raw`\text{cost \$} + y`]);
});

test("recognizes a bare aligned environment", () => {
  const text = String.raw`\begin{align}x &= 1 \\ y &= \frac{2}{3}\end{align}`;
  assert.deepEqual(equations(text), [{ type: "math", raw: text, expression: text, display: true }]);
});

test("the screenshot equation compiles to native MathML", () => {
  const expression = String.raw`x_t=(1-t)s+t\,x_{\text{target}}+0.5\sqrt{t(1-t)}\,\epsilon.`;
  const math = temml.renderToString(expression, { displayMode: true, throwOnError: true, trust: false });
  assert.match(math, /<math/);
  assert.match(math, /<msub>/);
  assert.match(math, /<msqrt>/);
  assert.match(math, /target/);
  assert.match(math, /ϵ|ε/);
  assert.ok(!math.includes("\\sqrt"));
});

test("fractions and matrices use MathML structure", () => {
  const math = temml.renderToString(String.raw`\frac{1}{2} + \begin{bmatrix}a & b \\ c & d\end{bmatrix}`, { throwOnError: true, trust: false });
  assert.match(math, /<mfrac>/);
  assert.match(math, /<mtable/);
});

test("invalid math throws for the reader's visible source fallback", () => {
  assert.throws(() => temml.renderToString(String.raw`\frac{1}`, { throwOnError: true }), /ParseError|Unexpected/);
});

test("untrusted math cannot introduce external links or images", () => {
  for (const expression of [String.raw`\href{https://example.com}{x}`, String.raw`\includegraphics{https://example.com/x.png}`]) {
    const math = temml.renderToString(expression, { trust: false, throwOnError: false });
    assert.ok(!/<a\b|<img\b|href=|src=/.test(math));
  }
});
