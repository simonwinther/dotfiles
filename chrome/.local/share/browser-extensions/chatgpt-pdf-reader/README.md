# ChatGPT PDF Reader

Press **Alt+Shift+Y** on ChatGPT to toggle a PDF-style reading view. Press the same shortcut again, click the reader's close button, or press **Escape** to return to the conversation. Clicking the extension's toolbar icon also toggles it.

The reader uses a Chrome-style dark PDF toolbar, gray canvas, white A4 pages, serif document text, page numbers, zoom, fit-to-width, and a document outline. Text remains selectable. It loads the full saved conversation, including older messages absent from the page, and shows the message count below the toolbar. Prompts retain their paragraphs and line breaks; responses render common Markdown headings, lists, tables, and code. LaTeX equations in both prompts and responses are typeset locally as MathML. Complete messages already rendered by ChatGPT retain their images and available native MathML equations. Invisible and unsupported placeholder characters are removed from the document title.

## Install

1. Open `chrome://extensions` and enable **Developer mode**.
2. Click **Load unpacked** and select `chrome/extensions/chatgpt-pdf-reader` inside this dotfiles checkout.
3. Open a ChatGPT conversation and press **Alt+Shift+Y**.

You can change the browser command at `chrome://extensions/shortcuts` under **Toggle PDF reader**. The page also listens directly for **Alt+Shift+Y**, including while typing or reading, so it can work in an installed ChatGPT app window where the browser command is not delivered. This page shortcut stays available even if you change the browser command. If your window manager intercepts it, choose an unused browser command. After editing these files, reload the extension on the extensions page.

Version 0.1.2 adds the page shortcut listener and a dedicated **Toggle PDF reader** browser command. Reload the extension to update it. The listener is installed in open conversations on reload; refresh the ChatGPT page if it is still unresponsive. The extension now requests access to the two supported ChatGPT sites for this listener. Alt+Shift+P from version 0.1.0 is reserved by Chromium for creating a tab group.

Version 0.1.3 supports conversation turn sections, older author wrappers, and semantic message content. This fixes an empty reader in ChatGPT layouts without `data-message-author-role` attributes.

Version 0.1.4 preserves prompt whitespace, cleans up titles, and follows the visible text when switching between the chat and reader.

Version 0.2.0 retrieves saved conversation history instead of relying on the messages mounted in the page. It follows pagination to the oldest page, supports the older conversation tree endpoint, excludes alternate branches, and reports loading failures instead of showing the welcome greeting or a partial transcript. Reload the extension and refresh the ChatGPT tab to update it.

Version 0.2.1 typesets inline and display LaTeX in prompts and responses, including older messages loaded from history. Equations are rendered before pagination. Code examples remain unchanged, and invalid expressions retain their source with a visible notice. Reload the extension and refresh ChatGPT after updating.

Version 0.2.2 cancels pending work and releases rendering state when the reader closes, including request timeouts, image waits, animation frames, and reader event listeners. Closing unrelated browser tabs no longer wakes the extension for bookkeeping.

This lives alongside the other extensions in the `chrome` Stow package. Load it from the checkout on each PC, as with the existing extensions; no machine-specific paths, build step, or manually created symlinks are needed.

## Behavior and limits

- The reader is an HTML recreation of a PDF viewer, not an actual PDF file. The browser address bar and tab title stay unchanged.
- It requests the current saved conversation from ChatGPT using your signed-in session and loads every older history page. For tree responses, it follows the selected branch back to the root. Hidden internal and tool messages are excluded. Shared conversations are supported when ChatGPT exposes their history. Temporary and unsaved chats are not supported; the reader explains this instead of pretending it loaded a full conversation. Toggle off and on to refresh after a response finishes streaming and is saved.
- A failed history request or an incomplete history response produces an explicit error. No partial transcript or unrelated welcome screen is substituted. Escape or the shortcut cancels loading and closes the reader.
- Opening the reader follows the visible line of text when that message is mounted and its complete rendered content can be reused. Closing can bring corresponding text back into view for those mounted messages. Reopening without moving the chat restores the reader's scroll position and zoom when its bookmark is still available. Older messages retrieved from history have no mounted scroll target. The draft stays intact. Different line wrapping, fixed headers, and the start/end of a scroll area can limit exact vertical alignment.
- The layout is isolated from ChatGPT's theme and page CSS. Existing accessibility or browser color filters can still affect its appearance.
- While the reader is closed, it has no polling, timers, animation frames, observers, rendering DOM, or ongoing history/image requests. Closing cancels pending work and releases the pages, text maps, image sources, and helper objects. A small bookmark and the browser's module cache may remain in memory; neither schedules work. The remaining page shortcut listener performs a small check on keypresses, so this is idle inactivity rather than a promise of literally zero CPU during every interaction. Chrome can suspend the [event-driven background worker](https://developer.chrome.com/docs/extensions/develop/concepts/service-workers/lifecycle) when it is idle.
- A small keyboard listener runs only on `chatgpt.com` and `chat.openai.com`. Conversation history is requested only when you invoke the reader. Session credentials are used in memory for requests to ChatGPT; no credentials or conversations are saved. There are no external services or analytics. Existing conversation images may be requested again by the browser when rendered.
- Equations use native MathML in complete mounted messages. Saved LaTeX is compiled with the included [Temml source modules](vendor/temml/README.md) and system math fonts. Supported delimiters are `$…$`, `$$…$$`, `\(…\)`, and `\[…\]`, plus common display environments such as `align` and `equation`. Code blocks and inline code are skipped. Unsupported or invalid equations retain their source, with an underlined fallback and a notice below the toolbar. No remote scripts, fonts, or equation services are loaded.
- Media not available in the mounted page appears as a labeled placeholder, with a notice below the toolbar. Interactive widgets are not reproduced. Raw HTML from saved messages remains inert text.
- History loading uses ChatGPT's private web endpoints, which may change. The paginated and legacy formats are also documented in this [conversation exporter implementation](https://github.com/sguzman/script-monkey/blob/main/userscripts/chatgpt.com/chatgpt-copy-entire-chat/chatgpt-copy-entire-chat.user.js).

Run the history and math regression tests with `node --test history.test.mjs math.test.mjs` (Node.js 20+). Browser rendering should also be checked after pagination or layout changes.

The optional `lifecycle.browser.mjs` check loads the unpacked extension in Chromium and verifies cleanup after closing during rendering, image decoding, or history loading. It also checks that resizing and typing while closed trigger no reader callbacks. Run with Playwright available as an installed package or via `PLAYWRIGHT_MODULE` pointing to its entry module. Chromium is found on `PATH`; `CHROMIUM_PATH` can override it. Test profiles are created in the system temporary directory.

The keyboard shortcut uses Chrome's [extension commands API](https://developer.chrome.com/docs/extensions/reference/api/commands).
