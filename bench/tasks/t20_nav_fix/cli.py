"""Order CLI demo entry (used by the t20_nav_fix mini-repo)."""

import sys

from utils.format import format_duration


def main() -> int:
    seconds = int(sys.argv[1]) if len(sys.argv) > 1 else 45
    print(format_duration(seconds))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
