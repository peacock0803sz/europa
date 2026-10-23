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

Then check that nbformat leaves its own output unchanged:

```fish
nix develop .#nbformat --command bash scripts/nbformat-rewrite.sh fixpoint
```

`fixpoint` rewrites every file in `jupyter/` again with nbformat and fails
unless the bytes stay the same. CI runs `golden`, expects no changes under
`fixtures/golden/`, and then runs `fixpoint`.

The insta snapshots in `snapshots/` that are built from the files in
`foreign/` and `jupyter/` fail `cargo test` until they are reviewed:

- `golden__golden-jupyter-*` for each changed file in `jupyter/`.
- `golden__golden-foreign-*` for each changed source in `foreign/`.
- `new_outputs__new-outputs-expected` when `jupyter/new_outputs.ipynb`
  changes, since it mirrors that file.
- `new_outputs__new-outputs-mixed` when `foreign/text_forms.ipynb` changes,
  since it is built from that file.

Run the tests and review each change on its own instead of accepting them
all at once. A snapshot should change the same way as the file it comes from.

```fish
cargo insta test -p europa-core
cargo insta review
```

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

Change the version in `scripts/nbformat-rewrite.sh`, in the CI workflow and
in [Pinned versions](#pinned-versions) in the same commit, regenerate, and
review the diff of `jupyter/` and the snapshots as described in
[Regenerating](#regenerating). Updating `flake.lock` can change the nbformat
that nixpkgs provides, which is the trigger for this procedure. The unstable
channel has already moved to nbformat 5.11.1, so the next lock update
requires it.
