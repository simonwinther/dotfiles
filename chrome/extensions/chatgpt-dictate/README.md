# ChatGPT Dictate Shortcuts

A minimal Chrome extension for ChatGPT dictation shortcuts.

While ChatGPT dictation is active, the extension asks the local native host to lower the default system output volume to 50%. The original volume is restored when dictation stops, is canceled, or the tab closes.

## Shortcuts

- `Alt+D`: start dictation. If dictation is already active, stop it, wait for the send button to become available, then submit.
- `Alt+C`: cancel active dictation.
- `Alt+S`: if dictating, finish dictation without submitting. If not dictating, submit the current prompt.

The content script also listens for these shortcuts directly on ChatGPT pages. That covers cases where Chrome's extension command dispatcher does not fire for the focused page.

## Install locally

1. Open `chrome://extensions`.
2. Enable **Developer mode**.
3. Click **Load unpacked**.
4. Select this folder: `/home/simon/dotfiles/chrome/extensions/chatgpt-dictate`.

The volume ducking bridge also needs the Chrome package and shell scripts stowed:

```bash
stow -n -v chrome usr-shell-scripts
stow -R chrome usr-shell-scripts
```

If Chrome still does not trigger `Alt+D`, open `chrome://extensions/shortcuts` and assign the shortcut manually. Some Chrome or operating-system shortcuts can take priority over extension shortcuts before the page can see them.
