# AGENTS.md

Personal GNU Stow dotfiles repo.

Rules:
- Never create symlinks manually with `ln`.
- Use Stow only.
- Do not touch unrelated files without very very explicitly asking first.
- Do not add binaries or generated files again without very explicity asking you're going to add.

Commands:
- `stow -n -v <package>`
- `stow -R <package>`
