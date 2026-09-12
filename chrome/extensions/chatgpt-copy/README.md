# Better Copy

A small Chrome extension that copies selected text and complete messages in your chosen format while preserving the original equation source.

## Install locally

1. Open `chrome://extensions`.
2. Enable Developer mode.
3. Choose Load unpacked.
4. Select this `chrome/extensions/chatgpt-copy` directory from your dotfiles checkout.

After updating the files, reload the extension on the extensions page and refresh any open conversation tabs once.

## Enable and choose a format

Click the extension's toolbar icon to open its settings:

- **Enabled:** choose either LaTeX or Markdown under Copy format.
- **Disabled:** the extension leaves clipboard handling to the page. The toolbar icon shows an `OFF` badge.
- Both settings apply immediately to open tabs and persist across browser restarts. Disabling the extension remembers your chosen format.

## LaTeX mode

LaTeX is the default format. Copies are document fragments ready to paste into an existing LaTeX document:

- Bold and italic text become `\textbf{...}` and `\emph{...}`.
- Headings become section commands; numbered heading prefixes are removed.
- Lists, quotations, and tables use `itemize` / `enumerate`, `quote`, and `tabular` environments.
- Inline code uses `\texttt{...}` and code blocks use `verbatim`.
- Special characters in prose are escaped without changing equation source.
- Links include their label and URL as text; images include their alternative text. Strikethrough text is retained without its decoration. These fallbacks avoid requiring extra formatting packages.
- Inline equations use `\(...\)`; display equations use `\[...\]`.

For example:

```tex
And \textbf{now} we introduce your Gaussian noise:

\[
\epsilon \sim \mathcal N(0,I).
\]
```

The destination document still needs any packages used by the original equations, such as `amsmath`.

## Markdown mode

Markdown mode uses Markdown headings, emphasis, lists, quotations, links, tables, and fenced code blocks. Inline equations use `$...$` and display equations use `$$...$$`, for Markdown editors that support math.

Both formats support selected-text copying and the message copy button. Selecting part of a rendered equation includes its complete source. Source attributes and older KaTeX annotations are both supported.
