# europa

## Installation

The crates are not published on crates.io, so install from a clone of this
repository:

```sh
cargo install --path crates/europa-tui
```

or straight from Git:

```sh
cargo install --git https://github.com/peacock0803sz/europa europa-tui
```

Building needs a C compiler (gcc or clang), because the syntax highlighting
links the C code of tree-sitter and its Python grammar statically into the
binary. Nothing besides the binary is needed at run time. The devShell of
`nix develop` includes a C compiler.
