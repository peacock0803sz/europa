# Golden Sources

Golden notebooks for byte-stable write-back.

## Layout

- `fixtures/golden/foreign/` holds the hand-written inputs. They use layouts
  and spellings that Jupyter would never produce.
- `fixtures/golden/jupyter/` holds the same notebooks rewritten by nbformat.
  These are generated files and are never edited by hand. Fix a source or the
  helper script and regenerate instead.

## Pinned versions

The expected output comes from nbformat 5.10.4 on Python 3.14. Locally both
come from the nixpkgs revision in `flake.lock`. CI installs the same nbformat
version with pip.

## Regenerating

From the repository root:

```fish
nix develop .#nbformat --command bash scripts/nbformat-rewrite.sh golden
```

The command deletes `jupyter/*.ipynb` and rewrites every file in `foreign/`
with `nbformat.read` and `nbformat.write`. Generate on Linux or macOS only.

## Rules for sources

- A source with `nbformat_minor` of 5 or higher gives every cell a unique `id`,
  so that nbformat does not insert random ids and the output stays
  reproducible.
- Every cell, including cells of an unknown type, has a `metadata` object.
  nbformat reads the `metadata` of all cells while stripping transient
  fields, and fails without it.
- Numbers in the new outputs of `new_outputs.ipynb` use the spelling of Python's
  `repr`, because the expected file is compared byte for byte with what
  Europa writes.
- Printable characters such as U+00E9, U+2028, U+2029 and emoji are written as
  `\u` escapes. Editors tend to turn them into raw characters, so check after
  writing: the escape must be present and the raw UTF-8 bytes must not be,
  using `LC_ALL=C grep -c -F` with the byte string.
- CRLF sources are written with LF first and converted with
  `awk '{ printf "%s\r\n", $0 }'`.

## Upgrading nbformat

Change the version in `scripts/nbformat-rewrite.sh` and in the CI workflow in
the same commit, regenerate, and review the diff of `jupyter/`. Updating
`flake.lock` can change the nbformat that nixpkgs provides, which is the
trigger for this procedure. The unstable channel has already moved to
nbformat 5.11.1, so the next lock update requires it.
