# Corpus Sources

Acceptance corpus for loading `.ipynb` files without loss.

## Sources

- `jupyter/nbformat` at `cfb73afdd2a7f358953f2a34f747fd0d0e9d6288`
  - Includes every `.ipynb` file under `tests/`.
- `jupyter/notebook` at `6cde9b9ddec821312b1ca16492233becfe145fca`
  - Includes every `.ipynb` file under `docs/source/examples/`.

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
