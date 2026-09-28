import sys
import os

sys.path.insert(0, os.path.dirname(os.path.abspath("_check.py")))

from utils.format import format_duration  # noqa: E402

assert format_duration(45) == "45s"
assert format_duration(90) == "1m30s"
assert format_duration(60) == "1m0s"
assert format_duration(3600) == "1h0m0s"
assert format_duration(0) == "0s"
assert format_duration(3725) == "1h2m5s"
print("ok")
