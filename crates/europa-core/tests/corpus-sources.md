# Corpus Sources

Acceptance corpus for loading `.ipynb` files without loss.

## Sources

- `jupyter/nbformat` at `cfb73afdd2a7f358953f2a34f747fd0d0e9d6288`
  - Includes every `.ipynb` file under `tests/`.
- `jupyter/notebook` at `6cde9b9ddec821312b1ca16492233becfe145fca`
  - Includes every `.ipynb` file in the checkout: the official examples
    under `docs/source/examples/` plus notebooks elsewhere such as
    `binder/` and `ui-tests/`.

Classification and the corpus test scan every `.ipynb` file under the
corpus directory, not only the paths named above.

## Fetching

Run from the repository root:

```sh
bash scripts/fetch-corpus.sh
```

The script clones both repositories at the pinned commits into
`target/europa-corpus/` by default and prints the value to set for
`EUROPA_CORPUS_DIR`. Override the destination with `EUROPA_CORPUS_DIR`
before running the script. CI runs the same command and then executes
the ignored corpus test with that directory set.

## Classification

`bash scripts/classify-corpus.sh` writes `corpus-expected.tsv` with one
tab-separated row per `.ipynb` file: relative path and `load`, `a`, `b`,
`c`, `d`, or `review`. The verdict follows the specification order:
raw-byte checks for (a) come before JSON parsing, then the root shape,
(b), (c), and the remaining (d) checks. Human overrides in
`corpus-overrides.tsv` win over generated rows. The script uses only
`jq`, `iconv`, and raw-byte inspection so expectations stay independent
from the Rust implementation.

Raw-byte checks for (a) cover a UTF-8 byte order mark, invalid UTF-8
sequences, unpaired surrogate escapes, and duplicate keys detected via
`jq --stream`. Depths from 120 to 136 levels and numeric spellings that
`jq` cannot distinguish (fractional forms of integer fields or integers
with 16 or more digits) become `review` for human confirmation. All
other files become `load` or the first matching reject reason.

## Rewriting with nbformat

`corpus_write` compares what Europa saves with what Jupyter saves, so the
comparison needs the corpus rewritten by nbformat. Run from the repository
root, after fetching the corpus:

```fish
nix develop .#nbformat --command bash scripts/nbformat-rewrite.sh corpus
EUROPA_JUPYTER_DIR=target/europa-corpus-jupyter cargo test -p europa-core --test corpus_write -- --ignored
```

The script reads every `load` row of `corpus-expected.tsv` from
`EUROPA_CORPUS_DIR` and writes the nbformat rewrite to the same relative path
under `EUROPA_JUPYTER_DIR` (default `target/europa-corpus-jupyter`).

Before it deletes the `.ipynb` files in the destination, the script resolves
both locations with `realpath -m` and stops without deleting anything unless
the destination lies below the repository `target/` directory and neither
location contains the other. Only `.ipynb` files inside the destination are
deleted.

The rewrite uses the pinned nbformat version in `scripts/nbformat-rewrite.sh`,
the same one that produces the golden notebooks. `validate` can add random cell
ids, which does not matter because the test compares Europa with the rewrite
itself. The rewrites are not committed.
