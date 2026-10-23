#!/usr/bin/env bash
# Rewrite notebooks with the reference nbformat implementation.
# Run from the repository root:
#   bash scripts/nbformat-rewrite.sh golden|corpus|fixpoint
set -euo pipefail

NBFORMAT_VERSION="5.10.4"
PYTHON="${PYTHON:-python3}"

GOLDEN_DIR="crates/europa-core/tests/fixtures/golden"
EXPECTED_TSV="crates/europa-core/tests/corpus-expected.tsv"

usage() {
  echo "usage: bash scripts/nbformat-rewrite.sh golden|corpus|fixpoint" >&2
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

# True when $1 equals $2 or lies below it.
is_within() {
  case "$1/" in
    "$2"/*) return 0 ;;
    *) return 1 ;;
  esac
}

corpus() {
  local src_root dest repo_target rel
  src_root="$(realpath -m "${EUROPA_CORPUS_DIR:-target/europa-corpus}")"
  dest="$(realpath -m "${EUROPA_JUPYTER_DIR:-target/europa-corpus-jupyter}")"
  repo_target="$(realpath -m target)"

  # The destination is emptied below, so refuse anything that could touch the
  # corpus itself or leave the repository build directory.
  if ! is_within "$dest" "$repo_target" || [ "$dest" = "$repo_target" ]; then
    echo "refusing: $dest is not below $repo_target" >&2
    exit 1
  fi
  if is_within "$dest" "$src_root" || is_within "$src_root" "$dest"; then
    echo "refusing: $dest and $src_root must not contain each other" >&2
    exit 1
  fi
  if [ ! -d "$src_root" ]; then
    echo "corpus not found at $src_root; run: bash scripts/fetch-corpus.sh" >&2
    exit 1
  fi

  mkdir -p "$dest"
  find "$dest" -type f -name '*.ipynb' -delete
  while IFS=$'\t' read -r rel verdict; do
    [ "$verdict" = "load" ] || continue
    printf '%s\t%s\n' "$src_root/$rel" "$dest/$rel"
  done <"$EXPECTED_TSV" | rewrite
}

fixpoint() {
  local tmp src
  tmp="$(mktemp -d)"
  # Expand now: $tmp is local and gone by the time the EXIT trap runs.
  trap "rm -rf '$tmp'" EXIT
  for src in "$GOLDEN_DIR"/jupyter/*.ipynb; do
    printf '%s\t%s\n' "$src" "$tmp/$(basename "$src")"
  done | rewrite
  for src in "$GOLDEN_DIR"/jupyter/*.ipynb; do
    if ! cmp -s "$src" "$tmp/$(basename "$src")"; then
      echo "not a fixed point: $src" >&2
      exit 1
    fi
  done
}

[ "$#" -eq 1 ] || usage
case "$1" in
  golden) golden ;;
  corpus) corpus ;;
  fixpoint) fixpoint ;;
  *) usage ;;
esac
