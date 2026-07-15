# Global Agent Instructions

## GitHub Authorship

- Never mention Claude, Codex, AI, agents, co-authoring, or generated-by attribution when creating GitHub content.
- Write commit messages, pull request text, and trailers as if the human author did the work directly.

## Clipboard Requests

### When to Copy

- Copy the exact requested content to the clipboard before responding whenever the user explicitly asks for something to be copied.
- Also treat clearly copy-ready requests—such as a single command, snippet, message, or prompt for ChatGPT—as clipboard requests.
- Trigger phrases include “copy this,” “give me something I can paste,” “one command,” and “one prompt.”

### How to Copy

- Run `wl-copy --foreground -- <content>` in a persistent exec session.
- Leave the session running so Wayland retains clipboard ownership until the user pastes or replaces the selection.
- Do not use a short-lived or default `wl-copy` invocation whose provider is terminated when the command wrapper exits.
