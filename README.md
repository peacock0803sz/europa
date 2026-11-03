# europa

## Installation

```sh
cargo install europa-tui
```

Building needs a C compiler (gcc or clang), because the syntax highlighting
links the C code of tree-sitter and its Python grammar statically into the
binary. Nothing besides the binary is needed at run time. The devShell of
`nix develop` includes a C compiler.
