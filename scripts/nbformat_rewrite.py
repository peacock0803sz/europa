"""Rewrite notebooks with the reference nbformat implementation.

Reads tab-separated `SRC<TAB>DST` lines from standard input. Each source is
read without version conversion and written to its destination, so the result
is what Jupyter itself would save.
"""

import argparse
import os
import sys

import nbformat


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--expect-version",
        required=True,
        help="nbformat version that must be installed",
    )
    args = parser.parse_args()

    if nbformat.__version__ != args.expect_version:
        print(
            f"nbformat version mismatch: expected {args.expect_version}, "
            f"found {nbformat.__version__}",
            file=sys.stderr,
        )
        return 1

    for line in sys.stdin:
        line = line.rstrip("\n")
        if not line:
            continue
        src, dst = line.split("\t", 1)
        try:
            notebook = nbformat.read(src, as_version=nbformat.NO_CONVERT)
            dst_dir = os.path.dirname(dst)
            if dst_dir:
                os.makedirs(dst_dir, exist_ok=True)
            nbformat.write(notebook, dst)
        except Exception as error:  # noqa: BLE001 - report any failure with its path
            print(f"{src}: {error}", file=sys.stderr)
            return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
