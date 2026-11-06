# Shared agent skills

Humanizer 3.1.0 is vendored from
https://github.com/blader/humanizer/tree/0ef9fe5bc3572734fd3ae6c448fe00a30cd5dcf0.
Its original skill instructions, interface metadata and MIT license are preserved.

The shared package manifest deploys it to `.agents/skills/humanizer` through
Home Manager on NixOS and GNU Stow on other distributions. Both machines use
the exact source revision recorded in the dotfiles repository.

Updates are explicit: review an upstream revision, replace the three source
files together, and update this revision and the package maintenance date.
Publish the dotfiles commit and update the NixOS dotfiles submodule pin before
rebuilding each machine. Upstream updates are not downloaded automatically.

The earlier manual installation in `.codex/skills/humanizer` should be backed
up outside skill-discovery directories when the managed installation is activated,
so the same skill is not loaded twice.
