#!/usr/bin/env bash
# Rewrite notebooks with the reference nbformat implementation.
# Run from the repository root: bash scripts/nbformat-rewrite.sh golden
set -euo pipefail

NBFORMAT_VERSION="5.10.4"
PYTHON="${PYTHON:-python3}"

GOLDEN_DIR="crates/europa-core/tests/fixtures/golden"

usage() {
  echo "usage: bash scripts/nbformat-rewrite.sh golden" >&2
  exit 2
}

rewrite() {
  "$PYTHON" scripts/nbformat_rewrite.py --expect-version "$NBFORMAT_VERSION"
}

golden() {
  local src
  find "$GOLDEN_DIR/jupyter" -type f -name '*.ipynb' -delete 2>/dev/null || true
  for src in "$GOLDEN_DIR"/foreign/*.ipynb; do
    printf '%s\t%s\n' "$src" "$GOLDEN_DIR/jupyter/$(basename "$src")"
  done | rewrite
}

[ "$#" -eq 1 ] || usage
case "$1" in
  golden) golden ;;
  *) usage ;;
esac
