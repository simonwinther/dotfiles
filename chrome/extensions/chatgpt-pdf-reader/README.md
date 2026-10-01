# ChatGPT PDF Reader

Press **Alt+Shift+Y** on ChatGPT to toggle a PDF-style reading view. Press the same shortcut again, click the reader's close button, or press **Escape** to return to the conversation. Clicking the extension's toolbar icon also toggles it.

The reader uses a Chrome-style dark PDF toolbar, gray canvas, white A4 pages, serif document text, page numbers, zoom, fit-to-width, and a document outline. Text remains selectable. Prompts retain their paragraphs and line breaks; responses retain headings, lists, tables, code, images, and available native MathML equations. Invisible and unsupported placeholder characters are removed from the document title.

## Install

1. Open `chrome://extensions` and enable **Developer mode**.
2. Click **Load unpacked** and select `chrome/extensions/chatgpt-pdf-reader` inside this dotfiles checkout.
3. Open a ChatGPT conversation and press **Alt+Shift+Y**.

You can change the browser command at `chrome://extensions/shortcuts` under **Toggle PDF reader**. The page also listens directly for **Alt+Shift+Y**, including while typing or reading, so it can work in an installed ChatGPT app window where the browser command is not delivered. This page shortcut stays available even if you change the browser command. If your window manager intercepts it, choose an unused browser command. After editing these files, reload the extension on the extensions page.

Version 0.1.2 adds the page shortcut listener and a dedicated **Toggle PDF reader** browser command. Reload the extension to update it. The listener is installed in open conversations on reload; refresh the ChatGPT page if it is still unresponsive. The extension now requests access to the two supported ChatGPT sites for this listener. Alt+Shift+P from version 0.1.0 is reserved by Chromium for creating a tab group.

Version 0.1.3 supports conversation turn sections, older author wrappers, and semantic message content. This fixes an empty reader in ChatGPT layouts without `data-message-author-role` attributes.

Version 0.1.4 preserves prompt whitespace, cleans up titles, and follows the visible text when switching between the chat and reader.

This lives alongside the other extensions in the `chrome` Stow package. Load it from the checkout on each PC, as with the existing extensions; no machine-specific paths, build step, or manually created symlinks are needed.

## Behavior and limits

- The reader is an HTML recreation of a PDF viewer, not an actual PDF file. The browser address bar and tab title stay unchanged.
- It takes a snapshot of conversation messages currently present in the page. Older messages that ChatGPT has not loaded are not included. Toggle off and on to refresh after a response finishes streaming.
- Opening the reader follows the visible line of text rather than the start of its message. Closing after scrolling in the reader brings the corresponding text into view in the chat. Reopening without moving the chat restores the reader's exact scroll position and zoom. The draft stays intact. Different line wrapping, fixed headers, and the start/end of a scroll area can limit exact vertical alignment.
- The layout is isolated from ChatGPT's theme and page CSS. Existing accessibility or browser color filters can still affect its appearance.
- A small keyboard listener runs only on `chatgpt.com` and `chat.openai.com`. Conversation text is read only when you invoke the reader. There are no services, analytics, dependencies, or saved conversations. Existing conversation images may be requested again by the browser when rendered.
- Equations use the page's native MathML when available, otherwise available source text. Interactive widgets are not reproduced.

The keyboard shortcut uses Chrome's [extension commands API](https://developer.chrome.com/docs/extensions/reference/api/commands).
