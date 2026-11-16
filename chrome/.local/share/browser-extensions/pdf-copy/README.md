# PDF Better Copy

A Chrome extension that opens PDFs in its own viewer and copies selected text with its equations as LaTeX or Markdown. It is the PDF counterpart of Better Copy in `../chatgpt-copy`.

Chrome's built-in PDF viewer cannot be extended, so this extension replaces it with a [PDF.js](https://mozilla.github.io/pdf.js/) viewer styled like Chrome's. A PDF has no LaTeX source, so the extension rebuilds it from the embedded fonts and glyph positions.

## Install

1. Open `chrome://extensions` and enable **Developer mode**.
2. Click **Load unpacked** and select `chrome/.local/share/browser-extensions/pdf-copy` in this dotfiles checkout.
3. Open the extension's **Details** and turn on **Allow access to file URLs** to use local PDFs such as files in `~/Downloads`.

After editing these files, reload the extension on the extensions page and reload open PDF tabs.

## Use

PDF links and local `.pdf` files open in the viewer automatically. Select text and press **Ctrl+C**. The format comes from the floating copy icon in the bottom-right corner: **LaTeX**, **Markdown**, or **Off**. Off copies plain text, as Chrome's viewer does. The toolbar popup has the same settings and a switch for opening PDFs in this viewer. Right-click a selection for **Copy as LaTeX**, **Copy as Markdown**, or **Copy as plain text** regardless of the current setting. A short notice confirms each copy.

For example, selecting the first exercise of a LaTeX assignment copies

```tex
Let \(\mathbf{v}\) be the vector \([-2, b, 7, 2, a]^T\). Compute \(\mathbf{v}^T\mathbf{v}\) and \(\mathbf{v}\mathbf{v}^T\).
```

or, as Markdown, `Let $\mathbf{v}$ be the vector $[-2, b, 7, 2, a]^T$. …`. Display equations become `\[ … \]` or `$$ … $$` blocks, as in Better Copy.

The toolbar provides page navigation, zoom (also **Ctrl+±**, **Ctrl+0**, and **Ctrl+scroll**), fit to page or width, rotation, find (**Ctrl+F**), download (**Ctrl+S**), and print (**Ctrl+P**). **Open in Chrome's PDF viewer** shows the current file in Chrome's own viewer, for forms, annotations, or anything else this viewer lacks. `#page=N` links open at that page.

## What is reconstructed

- Inline math with subscripts and superscripts, primes, Greek letters, operators, relations, arrows, and negated relations such as `\neq` and `\notin`.
- Fractions, roots including `\sqrt[n]{…}`, sums, products, integrals and `\lim`/`\max` with their limits, accents such as `\hat`, `\vec`, and `\widehat`, `\overline`, and `\underline`.
- Matrices as `bmatrix`, `pmatrix`, `vmatrix`, `Vmatrix`, or `Bmatrix`, including `\cdots`, `\vdots`, and `\ddots`; `cases`; tall `\left( … \right)` pairs; aligned equations as `align*` (`aligned` in Markdown).
- Letter styles: `\mathbf`, `\boldsymbol`, `\mathbb`, `\mathcal`, `\mathfrak`, `\mathrm`, `\mathsf`, `\mathtt`, and function names such as `\sin` or `\operatorname{rank}`.
- Section headings (numbers removed in LaTeX, like Better Copy), numbered and bulleted lists when their label is selected, bold, italic, and monospace text, and paragraphs with hyphenation at line ends removed.
- Special characters in prose are escaped for LaTeX; Markdown escapes `$`.

Selecting part of a formula copies the whole formula. Equation numbers and page numbers are left out.

Conversion works best with PDFs made by LaTeX: pdfLaTeX with Computer Modern or Latin Modern, newtx, Beamer slides, and LuaLaTeX or XeLaTeX with OpenType math fonts. The result is a reconstruction, so check it before relying on it:

- Spacing commands such as `\,` and `\quad`, macros, and labels cannot be recovered. A bar is read as `\mid` or `|` from its spacing.
- Tables, figures, and multi-column layouts are copied as text in the PDF's reading order.
- PDFs from word processors and web pages are converted from their Unicode text and may lose structure. Scanned PDFs have no text to copy.
- If a selection cannot be converted, the plain text is copied and the notice says so.

## Privacy and permissions

Everything runs locally. Besides fetching the PDF, the extension makes no requests and stores nothing but its settings.

- Host access to all sites: redirect PDF responses to the viewer and download them again for display. Chrome 128 or later is needed to match on the `Content-Type` header; downloads marked as attachments are left alone.
- `webNavigation`: local files never reach the network, so `file://` navigations ending in `.pdf` are redirected when they start.
- `declarativeNetRequestWithHostAccess`, `contextMenus`, and `storage`: the redirect rules, the copy menu, and the settings.

## Development

The conversion code lives in `lib/`: `page-data.js` reads text, math extension glyphs, and rules through PDF.js; `layout.js` builds fractions, roots, matrices, limits, and lines from geometry; `structure.js` separates math from prose and finds paragraphs, headings, and lists; `serialize.js` renders a selection. `lib/selection.js` maps the DOM selection to PDF.js text items.

Run the regression tests with `node --test tests/*.test.mjs` (Node.js 22+). They load the LaTeX fixtures in `tests/fixtures` with the vendored PDF.js and compare whole pages with `tests/expected`. Set `UPDATE_SNAPSHOTS=1` to rewrite the snapshots after an intended change, and review the diff. Rebuild the fixture PDFs with `tests/build-fixtures.sh` (needs TeX Live with LuaLaTeX).

The optional `tests/viewer.browser.mjs` loads the unpacked extension in Chromium, opens a PDF over HTTP, and checks the redirect, LaTeX, Markdown, and plain copies, the toolbar, the popup, and handing the PDF back to Chrome's viewer. Run it with Playwright installed or `PLAYWRIGHT_MODULE` pointing to its entry module. Chromium is found on `PATH`; `CHROMIUM_PATH` overrides it.

PDF.js 6.5.438 from the `pdfjs-dist` package is vendored in `vendor/pdfjs` under its Apache 2.0 license. To update it, copy `build/pdf.min.mjs`, `build/pdf.worker.min.mjs`, `web/pdf_viewer.mjs`, `web/pdf_viewer.css`, `web/images`, `cmaps`, `standard_fonts`, `iccs`, and the JPX, JBIG2, and QCMS files from `wasm` of the new release, then rerun both tests.
