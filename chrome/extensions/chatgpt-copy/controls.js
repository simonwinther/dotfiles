(() => {
  function mountControls() {
    if (document.getElementById("better-copy-controls")) {
      return;
    }

    const host = document.createElement("div");
    host.id = "better-copy-controls";
    host.setAttribute("data-copy-ignore", "true");
    const root = host.attachShadow({ mode: "open" });
    root.innerHTML = `
      <style>
        :host {
          all: initial;
          --surface: #ffffff;
          --text: #27272a;
          --muted: #71717a;
          --border: #e4e4e7;
          --hover: #f4f4f5;
          --active: #ecfdf5;
          --accent: #047857;
          --error: #b91c1c;
          color-scheme: light;
          position: fixed;
          right: max(20px, env(safe-area-inset-right));
          bottom: max(20px, env(safe-area-inset-bottom));
          z-index: 2147483646;
          font: 13px/1.45 system-ui, -apple-system, BlinkMacSystemFont, sans-serif;
          color: var(--text);
          text-align: left;
        }

        @media (prefers-color-scheme: dark) {
          :host {
            --surface: #242426;
            --text: #f4f4f5;
            --muted: #a1a1aa;
            --border: #3f3f46;
            --hover: #303034;
            --active: #163b32;
            --accent: #6ee7b7;
            --error: #fca5a5;
            color-scheme: dark;
          }
        }

        :host-context(html.dark) {
          --surface: #242426;
          --text: #f4f4f5;
          --muted: #a1a1aa;
          --border: #3f3f46;
          --hover: #303034;
          --active: #163b32;
          --accent: #6ee7b7;
          --error: #fca5a5;
          color-scheme: dark;
        }

        :host-context(html.light) {
          --surface: #ffffff;
          --text: #27272a;
          --muted: #71717a;
          --border: #e4e4e7;
          --hover: #f4f4f5;
          --active: #ecfdf5;
          --accent: #047857;
          --error: #b91c1c;
          color-scheme: light;
        }

        * { box-sizing: border-box; }
        button, input { font: inherit; }
        button { color: inherit; cursor: pointer; }
        button:focus-visible, input:focus-visible {
          outline: 2px solid var(--accent);
          outline-offset: 4px;
        }
        #launcher {
          position: relative;
          display: grid;
          place-items: center;
          width: 36px;
          height: 36px;
          padding: 0;
          border: 1px solid var(--border);
          border-radius: 50%;
          background: var(--surface);
          box-shadow: 0 3px 14px #0002;
          transition: background 160ms ease, transform 160ms ease;
        }
        #launcher:hover { background: var(--hover); transform: translateY(-2px); }
        #badge {
          position: absolute;
          right: -5px;
          bottom: -3px;
          min-width: 25px;
          padding: 1px 4px;
          border: 2px solid var(--surface);
          border-radius: 6px;
          background: var(--active);
          color: var(--accent);
          font-size: 9px;
          font-weight: 700;
          line-height: 13px;
        }
        #badge[data-off] { background: var(--hover); color: var(--muted); }
        #panel {
          position: absolute;
          right: 0;
          bottom: 48px;
          width: 196px;
          max-width: calc(100vw - 40px);
          max-height: calc(100dvh - 100px);
          overflow-y: auto;
          padding: 10px;
          border: 1px solid var(--border);
          border-radius: 12px;
          background: var(--surface);
          box-shadow: 0 12px 40px #0003;
          opacity: 0;
          visibility: hidden;
          transform: translateY(8px) scale(0.97);
          transform-origin: bottom right;
          transition: opacity 160ms ease, transform 160ms ease, visibility 0s linear 160ms;
        }
        #panel[data-open] { opacity: 1; visibility: visible; transform: none; transition-delay: 0s; }
        header { display: flex; align-items: center; justify-content: space-between; margin-bottom: 6px; }
        h2 { margin: 0; font-size: 13px; font-weight: 650; }
        #close {
          display: grid;
          place-items: center;
          width: 28px;
          height: 28px;
          margin: -4px -4px -4px 0;
          padding: 0;
          border: 0;
          border-radius: 50%;
          background: transparent;
        }
        #close:hover { background: var(--hover); }
        fieldset { min-width: 0; margin: 0; padding: 0; border: 0; }
        legend, #status {
          position: absolute;
          width: 1px;
          height: 1px;
          margin: 0;
          overflow: hidden;
          clip-path: inset(50%);
        }
        label {
          display: flex;
          align-items: center;
          gap: 10px;
          min-height: 36px;
          margin-top: 3px;
          padding: 7px 10px;
          border: 1px solid transparent;
          border-radius: 8px;
          cursor: pointer;
          transition: background 120ms ease;
        }
        label:hover { background: var(--hover); }
        label:has(input:checked) { background: var(--active); border-color: var(--accent); }
        input { flex: none; width: 14px; height: 14px; margin: 0; accent-color: var(--accent); cursor: pointer; }
        strong { display: block; font-size: 12px; font-weight: 600; }
        fieldset:disabled { opacity: 0.5; }
        fieldset:disabled label, input:disabled { cursor: default; }
        #error { margin: 10px 0 0; color: var(--error); font-size: 12px; }
        @media (prefers-reduced-motion: reduce) {
          *, *::before, *::after { transition: none !important; }
        }
      </style>
      <button id="launcher" type="button" aria-label="Open copy settings"
        aria-expanded="false" aria-controls="panel" title="Better Copy">
        <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor"
          stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <rect x="8" y="8" width="12" height="13" rx="2"></rect>
          <path d="M16 8V5a2 2 0 0 0-2-2H5a2 2 0 0 0-2 2v10a2 2 0 0 0 2 2h3"></path>
        </svg>
        <span id="badge" aria-hidden="true">…</span>
      </button>
      <section id="panel" aria-labelledby="heading" inert>
        <header>
          <h2 id="heading">Better Copy</h2>
          <button id="close" type="button" aria-label="Close copy settings">
            <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor"
              stroke-width="1.5" stroke-linecap="round" aria-hidden="true">
              <path d="m4 4 8 8M12 4l-8 8"></path>
            </svg>
          </button>
        </header>
        <fieldset id="modes" disabled>
          <legend>Copy mode</legend>
          <label title="Text and equations for your document">
            <input type="radio" name="mode" value="latex" aria-describedby="latex-hint">
            <strong>LaTeX</strong>
            <span id="latex-hint" hidden>Text and equations for your document</span>
          </label>
          <label title="Markdown with math delimiters">
            <input type="radio" name="mode" value="markdown" aria-describedby="markdown-hint">
            <strong>Markdown</strong>
            <span id="markdown-hint" hidden>Markdown with math delimiters</span>
          </label>
          <label title="Use the page’s normal copy behavior">
            <input type="radio" name="mode" value="off" aria-describedby="off-hint">
            <strong>Off</strong>
            <span id="off-hint" hidden>Use the page’s normal copy behavior</span>
          </label>
        </fieldset>
        <p id="status" role="status">Loading settings…</p>
        <p id="error" role="alert" hidden></p>
      </section>
    `;
    document.documentElement.appendChild(host);

    const launcher = root.getElementById("launcher");
    const panel = root.getElementById("panel");
    const badge = root.getElementById("badge");
    const picker = root.getElementById("modes");
    const status = root.getElementById("status");
    const error = root.getElementById("error");
    const inputs = [...root.querySelectorAll('input[name="mode"]')];
    let settings = { enabled: true, format: "latex" };
    let loaded = false;
    let saving = false;

    function render() {
      const mode = settings.enabled ? settings.format : "off";
      const name = mode === "latex" ? "LaTeX" : mode === "markdown" ? "Markdown" : "Off";
      picker.disabled = !loaded;
      picker.setAttribute("aria-busy", String(saving));
      for (const input of inputs) {
        input.checked = loaded && input.value === mode;
      }
      badge.textContent = loaded ? { latex: "TeX", markdown: "MD", off: "OFF" }[mode] : "…";
      badge.toggleAttribute("data-off", mode === "off");
      launcher.title = loaded ? `Better Copy · ${name}` : "Better Copy";
      launcher.setAttribute("aria-label", `${launcher.title}. Copy settings`);
      status.textContent = saving ? "Saving…" : loaded ? `Current mode: ${name}` : "Loading settings…";
    }

    function setOpen(open, restoreFocus = false) {
      panel.toggleAttribute("data-open", open);
      panel.inert = !open;
      launcher.setAttribute("aria-expanded", String(open));
      if (open) {
        (inputs.find((input) => input.checked) || root.getElementById("close")).focus();
        if (!loaded) loadSettings();
      } else if (restoreFocus) {
        launcher.focus();
      }
    }

    function showError(message) {
      error.textContent = message;
      error.hidden = false;
    }

    function loadSettings() {
      try {
        chrome.storage.local.get({ enabled: true, format: "latex" }, (items) => {
          if (chrome.runtime.lastError) {
            status.textContent = "Settings unavailable";
            showError("Could not load settings. Close and reopen to retry.");
            return;
          }
          settings = {
            enabled: items.enabled !== false,
            format: items.format === "markdown" ? "markdown" : "latex"
          };
          loaded = true;
          error.hidden = true;
          render();
        });
      } catch {
        status.textContent = "Settings unavailable";
        showError("Refresh this page to reconnect the extension.");
      }
    }

    function saveMode(mode) {
      if (!loaded || saving) {
        render();
        return;
      }
      const changes = mode === "off" ? { enabled: false } : { enabled: true, format: mode };
      const previous = settings;
      settings = { ...settings, ...changes };
      saving = true;
      error.hidden = true;
      render();

      function finishSave(failed) {
        saving = false;
        if (failed) {
          settings = previous;
          showError("Could not save settings. Try again or refresh this page.");
        }
        render();
      }

      try {
        chrome.storage.local.set(changes, () => finishSave(Boolean(chrome.runtime.lastError)));
      } catch {
        finishSave(true);
      }
    }

    launcher.addEventListener("click", () => setOpen(!panel.hasAttribute("data-open")));
    root.getElementById("close").addEventListener("click", () => setOpen(false, true));
    picker.addEventListener("change", (event) => {
      if (inputs.includes(event.target) && event.target.checked) saveMode(event.target.value);
    });
    document.addEventListener("pointerdown", (event) => {
      if (!event.composedPath().includes(host)) setOpen(false);
    }, true);
    document.addEventListener("focusin", (event) => {
      // Wait for outside focus; label clicks briefly clear focus before activating their radio.
      if (!event.composedPath().includes(host)) setOpen(false);
    });
    root.addEventListener("keydown", (event) => {
      if (event.key === "Escape" && panel.hasAttribute("data-open")) {
        event.preventDefault();
        event.stopPropagation();
        setOpen(false, true);
      }
    });

    try {
      chrome.storage.onChanged.addListener((changes, area) => {
        if (area !== "local") return;
        if (changes.enabled) settings.enabled = changes.enabled.newValue !== false;
        if (changes.format) settings.format = changes.format.newValue === "markdown" ? "markdown" : "latex";
        render();
      });
    } catch {
      // Loading settings below also reports a disconnected extension.
    }
    loadSettings();
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", mountControls, { once: true });
  } else {
    mountControls();
  }
})();
