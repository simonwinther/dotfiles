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
