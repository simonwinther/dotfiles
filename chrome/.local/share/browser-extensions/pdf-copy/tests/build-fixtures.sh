#!/usr/bin/env bash
# Rebuilds the test PDFs from their LaTeX sources (needs TeX Live with LuaLaTeX).
set -euo pipefail
cd "$(dirname "$0")/fixtures"
build=$(mktemp -d)
trap 'rm -rf "$build"' EXIT
for source in *.tex; do
  engine=pdflatex
  if head -n1 "$source" | grep -q lualatex; then engine=lualatex; fi
  for _ in 1 2; do "$engine" -interaction=nonstopmode -halt-on-error -output-directory="$build" "$source" >/dev/null; done
  cp "$build/${source%.tex}.pdf" .
done
