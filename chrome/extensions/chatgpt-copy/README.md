# ChatGPT LaTeX Copy

A small Chrome extension that rewrites ChatGPT clipboard output so rendered KaTeX equations copy as raw LaTeX.

## Install locally

1. Open `chrome://extensions`.
2. Enable Developer mode.
3. Choose Load unpacked.
4. Select this directory: `/home/simon/dotfiles/chrome/extensions/chatgpt-copy`.

## Behavior

- Selecting text in ChatGPT and copying it writes readable text with raw LaTeX equations to the clipboard.
- ChatGPT-rendered equations are copied from KaTeX annotations, for example:

```tex
\[
L_T(h) \le \exp(-2\gamma^2 B).
\]
```

- ChatGPT's message copy button is also intercepted and writes the answer with raw LaTeX equations.
- Code blocks are copied as fenced Markdown code blocks.
- Headings copy as LaTeX sections: `#` to `\section{}`, `##` to `\subsection{}`, and `###` to `\subsubsection{}`. Deeper headings stay as Markdown hashes.
