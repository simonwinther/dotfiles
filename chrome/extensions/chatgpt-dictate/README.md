# ChatGPT Dictate Shortcuts

A minimal Chrome extension for ChatGPT dictation shortcuts.

While ChatGPT dictation is active, the extension asks the local native host to lower the default system output volume to 30%. The original volume is restored when dictation stops, is canceled, or the tab closes.

## Shortcuts

- `Alt+D`: start dictation. If dictation is already active, stop it, wait for the send button to become available, then submit.
- `Alt+C`: cancel active dictation.
- `Alt+S`: if dictating, finish dictation without submitting. If not dictating, submit the current prompt.
- `Alt+M`: toggle the microphone mute button during a live voice conversation. Press again to unmute. Does nothing when no microphone mute control is available.

The content script also listens for these shortcuts directly on ChatGPT pages. That covers cases where Chrome's extension command dispatcher does not fire for the focused page.

## Install locally

1. Open `chrome://extensions`.
2. Enable **Developer mode**.
3. Click **Load unpacked**.
4. Select this folder: `/home/simon/dotfiles/chrome/extensions/chatgpt-dictate`.

After updating, reload the extension on `chrome://extensions` and refresh any open conversation tabs. If `Alt+M` is unassigned, set it under `chrome://extensions/shortcuts`.

The volume ducking bridge also needs the Chrome package and shell scripts stowed:

```bash
stow -n -v chrome usr-shell-scripts
stow -R chrome usr-shell-scripts
```

If Chrome still does not trigger `Alt+D`, open `chrome://extensions/shortcuts` and assign the shortcut manually. Some Chrome or operating-system shortcuts can take priority over extension shortcuts before the page can see them.
