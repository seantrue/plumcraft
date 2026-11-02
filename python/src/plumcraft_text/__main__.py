"""`python -m plumcraft_text`: the text engine as a mode 2 process. The UI spawns it with
`PLUMCRAFT_ENGINE_SOCKET` and `PLUMCRAFT_ENGINE_TOKEN` set (or pass `--connect unix:<path>`;
`--listen host:port` serves mode 3)."""

import sys

from plumcraft_text import TextEngine


def main() -> int:
    return TextEngine.serve()


if __name__ == "__main__":
    sys.exit(main())
