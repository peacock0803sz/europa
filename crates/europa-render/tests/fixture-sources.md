# Fixture Sources

Inputs for the render snapshots in `render_snapshots.rs`.

## Shape

Each file in `fixtures/` holds the rendering conditions and the outputs of one
code cell, with the outputs in the form nbformat saves them:

```json
{ "options": { "width": 40 }, "outputs": [] }
```

`options` is a `RenderOptions` as serde reads it, so `max_output_lines` and
`background` can be left out.

## Sources

- `traceback-ipython.json` copies `cells[0].outputs[0]` of
  `tests/many_tracebacks.ipynb` in `jupyter/nbformat` at
  `cfb73afdd2a7f358953f2a34f747fd0d0e9d6288`, the commit that
  `scripts/fetch-corpus.sh` pins. IPython wrote that traceback, so it has the
  color sequences a real one has. nbformat is licensed under BSD 3-Clause.
  After fetching the corpus, the file can be made again with:

  ```fish
  jq '{options: {width: 80}, outputs: [.cells[0].outputs[0]]}' target/europa-corpus/nbformat/tests/many_tracebacks.ipynb
  ```

- The other fixtures are written by hand.
