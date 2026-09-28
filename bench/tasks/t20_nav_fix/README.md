# order-cli

Mini order tool.

## Usage

    python cli.py <seconds>

Prints the elapsed duration for an order stage. Operations reported the
display is wrong for short stages: `python cli.py 45` prints `0m45s`
but the expected output is `45s`.
