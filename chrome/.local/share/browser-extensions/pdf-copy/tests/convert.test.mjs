// Run with: node --test tests/*.test.mjs  (set UPDATE_SNAPSHOTS=1 to rewrite tests/expected)
import assert from "node:assert/strict";
import fs from "node:fs";
import { after, test } from "node:test";
import { copy, destroyAll, loadPage, pageCount, select } from "./helpers.mjs";

after(destroyAll);

const EXPECTED = new URL("expected/", import.meta.url);

for (const fixture of ["article.pdf", "opentype.pdf", "newtx.pdf", "slides.pdf"]) {
  test(`whole pages of ${fixture} match their snapshots`, async () => {
    const pages = await pageCount(fixture);
    for (let pageNumber = 1; pageNumber <= pages; pageNumber++) {
      const model = await loadPage(fixture, pageNumber);
      for (const [format, extension] of [["latex", "tex"], ["markdown", "md"]]) {
        const actual = `${copy(model, select(model), format)}\n`;
        const file = new URL(`${fixture.replace(/\.pdf$/, "")}-${pageNumber}.${extension}`, EXPECTED);
        if (process.env.UPDATE_SNAPSHOTS) {
          fs.mkdirSync(EXPECTED, { recursive: true });
          fs.writeFileSync(file, actual);
        } else {
          assert.equal(actual, fs.readFileSync(file, "utf8"), `${fixture} page ${pageNumber} as ${format}`);
        }
      }
    }
  });
}

test("a selection inside a list item copies inline math", async () => {
  const model = await loadPage("article.pdf", 1);
  const selection = select(model, "1. L|et", "vvT .");
  assert.equal(copy(model, selection),
    "et \\(\\mathbf{v}\\) be the vector \\([-2, b, 7, 2, a]^T\\). Compute \\(\\mathbf{v}^T\\mathbf{v}\\) and \\(\\mathbf{v}\\mathbf{v}^T\\).");
  assert.equal(copy(model, selection, "markdown"),
    "et $\\mathbf{v}$ be the vector $[-2, b, 7, 2, a]^T$. Compute $\\mathbf{v}^T\\mathbf{v}$ and $\\mathbf{v}\\mathbf{v}^T$.");
});

test("selecting part of a formula copies all of it", async () => {
  const model = await loadPage("article.pdf", 1);
  assert.equal(copy(model, select(model, "Let x|", "ij")), "\\(x_i^2 + y_{ij} = e^{x^2}\\)");
  assert.equal(copy(model, select(model, "Compute v = B.|[", "x, y")), "\\(\\mathbf{v} = B.[x, y]^T\\)");
});

test("a list item with its label becomes a list", async () => {
  const model = await loadPage("article.pdf", 1);
  const latex = copy(model, select(model, "|3. Let A", "nilpotent"));
  assert.match(latex, /^\\begin\{enumerate\}\n\\setcounter\{enumi\}\{2\}\n\\item Let \\\(A\\\) be the matrix:/);
  assert.match(latex, /\\begin\{bmatrix\}\n1 & -1 & -1 \\\\\n8 - x & x - 6 & x - 6 \\\\\nx - 7 & 5 - x & 5 - x\n\\end\{bmatrix\}/);
  const markdown = copy(model, select(model, "|3. Let A", "nilpotent"), "markdown");
  assert.match(markdown, /^3\. Let \$A\$ be the matrix:\n\n {3}\$\$\n {3}A = \\begin\{bmatrix\}/);
});

test("prose characters are escaped for each format", async () => {
  const model = await loadPage("article.pdf", 1);
  const selection = select(model, "|50", "score");
  assert.equal(copy(model, selection), "50\\% of \\$5 \\& \\#1 under\\_score");
  assert.equal(copy(model, selection, "markdown"), "50% of \\$5 & #1 under_score");
});

test("display equations drop their equation numbers", async () => {
  const model = await loadPage("article.pdf", 2);
  assert.equal(copy(model, select(model, "|softmax", "(1)")),
    "\\[\n\\operatorname{softmax}\\left( \\frac{QK^T}{\\sqrt{d_k}} \\right)V = \\lim_{n\\to\\infty} \\max_x \\det A\n\\]");
});

test("page numbers are left out when text around them is selected", async () => {
  const model = await loadPage("article.pdf", 2);
  assert.doesNotMatch(copy(model, select(model, "|Second item")), /\n2$/);
});
