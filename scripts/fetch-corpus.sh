#!/usr/bin/env bash
# Fetch acceptance corpus for notebook loading tests.
# Pinned commits keep results reproducible across machines and CI.
set -euo pipefail

NBFORMAT_REPO="https://github.com/jupyter/nbformat.git"
NBFORMAT_COMMIT="cfb73afdd2a7f358953f2a34f747fd0d0e9d6288"
NOTEBOOK_REPO="https://github.com/jupyter/notebook.git"
NOTEBOOK_COMMIT="6cde9b9ddec821312b1ca16492233becfe145fca"

CORPUS_DIR="${EUROPA_CORPUS_DIR:-target/europa-corpus}"
NBFORMAT_DIR="$CORPUS_DIR/nbformat"
NOTEBOOK_DIR="$CORPUS_DIR/notebook"

fetch_one() {
  local repo="$1"
  local commit="$2"
  local dest="$3"

  if [ -d "$dest/.git" ]; then
    (cd "$dest" && git fetch --quiet origin "$commit")
  else
    mkdir -p "$(dirname "$dest")"
    git clone --quiet "$repo" "$dest"
    (cd "$dest" && git fetch --quiet origin "$commit")
  fi
  (cd "$dest" && git checkout --quiet "$commit")
}

fetch_one "$NBFORMAT_REPO" "$NBFORMAT_COMMIT" "$NBFORMAT_DIR"
fetch_one "$NOTEBOOK_REPO" "$NOTEBOOK_COMMIT" "$NOTEBOOK_DIR"

echo "EUROPA_CORPUS_DIR=$CORPUS_DIR"
