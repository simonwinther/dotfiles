# AGENTS.md

Personal GNU Stow dotfiles repo.

Rules:
- Never create symlinks manually with `ln`.
- Use Stow only.
- Put personal desktop tools and their configuration in Stow packages here, so they are reproducible on both PCs. Keep paths independent of the machine and username.
- Do not touch unrelated files without very very explicitly asking first.
- Do not add binaries or generated files again without very explicity asking you're going to add.

Commands:
- `stow -n -v <package>`
- `stow -R <package>`

## Update tracker

- Read `LAST_UPDATED` when asked to update old or stale components. Dates use `DD/MM/YYYY`; compare them with today's local date.
- Keep one alphabetically sorted `component: DD/MM/YYYY` line per package or tool. Package names match their directories.
- After changing a component, set its date to today's local date and include the tracker change in the same commit. Change only the entries affected by the work; checking a component does not reset its date.
- Add an entry for a new package or tool, and remove it when the component is removed.
- `pdf-picker` covers `hypr/.local/bin/pdf-picker` and PDF mode in `file-picker/.local/bin/file-picker`. Shared picker changes update both `pdf-picker` and `file-picker`; update `hypr` only when files in that package change.
