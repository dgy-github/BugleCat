from jsonpath import get

d = {"a": {"b": [10, {"c": 42}]}, "k": 0, "empty": "", "none": None}

assert get(d, "a.b.0") == 10
assert get(d, "a.b.1.c") == 42
assert get(d, "a.x", "NA") == "NA"
assert get(d, "a.b.9", -1) == -1
assert get(d, "a.b.1.z") is None              # missing key -> default (None)
assert get(d, "a") == {"b": [10, {"c": 42}]}  # returns subtree
assert get(d, "k") == 0                        # falsy scalar, not default
assert get(d, "empty") == ""                   # falsy scalar, not default
assert get(d, "none", "DEF") is None           # value IS None -> return None
assert get(d, "a.b.notanindex", "NA") == "NA"  # key segment on a list
assert get(d, "a.0", "NA") == "NA"             # index segment on a dict
assert get(d, "missing", "NA") == "NA"

print("ok")
