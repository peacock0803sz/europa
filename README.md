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

## Usage

Open a notebook with:

```sh
europa notebook.ipynb
```

The first screen shows the cells from the top. Quit with `q` or `Ctrl-C`.
The exit codes are 0 for success and `--help`, 64 for usage errors, 65 for
a notebook that cannot be parsed, 66 for a file that cannot be read, 74 for
a terminal that cannot be used, 129 for SIGHUP and 143 for SIGTERM.

| Keys | Move |
|------|------|
| `j`, `↓` | One line down |
| `k`, `↑` | One line up |
| `]` | Next cell |
| `[` | Previous cell |
| `gg` | First cell |
| `G` | Last cell |
| `q`, `Ctrl-C` | Quit |

At startup the terminal is asked for its background color in the same
single query as the graphics protocol. When it answers, light or dark
colors are chosen for stderr, errors and code blocks. Without an answer
the background is dark. Terminals that answer in two digits read as dark,
because the query scales 16-bit values down without knowing their digits.

## Images

PNG and JPEG outputs are shown inline, in the place where they were produced.
The terminal is asked which graphics protocol it supports (Kitty, Sixel or
iTerm2), and the answer is used when there is one. Terminals that do not
support any, or that do not answer, get half-block characters, which work
everywhere and need no query. The probe can skip the query by choosing
half-blocks by hand, as does a session whose input or output is not a terminal.
In `europa` the same images are scrolled with `j` and `k`.

An image that cannot be read, or that is over 64 million pixels, is named in a
line of its own with the reason, for example `[image/png: broken]`, and the
outputs around it are drawn as usual. If a protocol fails while drawing, the
image is drawn with half-blocks instead.

Inside tmux, `allow-passthrough` is turned on for the pane when the picker is
created, because Kitty, Sixel and iTerm2 images reach the terminal through it.
This also happens when the tests run inside tmux.

A terminal that counts characters of ambiguous width as two columns can shift
the columns of an image drawn with half-blocks, because the half-block
characters are of ambiguous width.

To look at each protocol on a real terminal, build the probe and open the
sample notebook it writes:

```sh
cargo build --release -p europa-tui --example image_probe
target/release/examples/image_probe --write-sample sample.ipynb
target/release/examples/image_probe sample.ipynb --protocol halfblock
```

`j` and `k` scroll and `q` quits. Without `--protocol` the terminal decides.

## Development

The kernel integration tests need a Python with `ipykernel` and never run
inside plain `cargo test`. Point `EUROPA_KERNEL_PYTHON` at such an
interpreter and run the ignored tests serially:

```sh
EUROPA_KERNEL_PYTHON=/path/to/python cargo test -p europa-kernel --test local_kernel -- --ignored --test-threads=1
```

Europa never installs packages into your Python environment; bring your
own `ipykernel`.

Europa stops kernels on every exit path it controls. A normal shutdown asks
the kernel to exit and kills its process group only after the grace period,
dropping the kernel value kills the group at once, and whenever a kernel
ends, whatever is left of its group is killed too. `kill_all_kernels` is the
best-effort cleanup meant for panic hooks and signal handling. If Europa
itself is killed with `SIGKILL`, the kernel still ends through the Linux
parent-death signal and, on both Linux and macOS, the `JPY_PARENT_PID` watch
in `ipykernel`, which checks about once a second. Processes the kernel
started and its connection file are left behind in that case.
