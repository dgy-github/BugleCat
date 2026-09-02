import basen

# ---- to_base basics ----
assert basen.to_base(0, 2) == "0"
assert basen.to_base(0, 36) == "0"
assert basen.to_base(5, 2) == "101"
assert basen.to_base(255, 16) == "ff"
assert basen.to_base(-255, 16) == "-ff"
assert basen.to_base(35, 36) == "z"
assert basen.to_base(36, 36) == "10"
assert basen.to_base(10, 10) == "10"
assert basen.to_base(-1, 2) == "-1"
assert basen.to_base(1, 2) == "1"

# no leading zeros, no plus
assert basen.to_base(7, 8) == "7"
assert not basen.to_base(8, 8).startswith("0")
assert "+" not in basen.to_base(123456789, 16)

# zero never gets a sign
assert basen.to_base(0, 10) == "0"

# large value sanity vs builtin hex
assert basen.to_base(0xDEADBEEF, 16) == "deadbeef"

# ---- to_base errors ----
for bad in [1, 0, -3, 37, 100]:
    try:
        basen.to_base(10, bad)
        assert False, "expected ValueError for base %r" % bad
    except ValueError:
        pass

# base must be int -> TypeError (and bool base rejected)
for bad in [16.0, "16", None, True, False]:
    try:
        basen.to_base(10, bad)
        assert False, "expected TypeError for base %r" % (bad,)
    except TypeError:
        pass

# n must be int, not bool, not other types -> TypeError
for bad in [1.0, "5", None, True, False]:
    try:
        basen.to_base(bad, 10)
        assert False, "expected TypeError for n %r" % (bad,)
    except TypeError:
        pass

# ---- from_base basics ----
assert basen.from_base("0", 2) == 0
assert basen.from_base("101", 2) == 5
assert basen.from_base("ff", 16) == 255
assert basen.from_base("FF", 16) == 255
assert basen.from_base("Ff", 16) == 255
assert basen.from_base("-ff", 16) == -255
assert basen.from_base("+ff", 16) == 255
assert basen.from_base("z", 36) == 35
assert basen.from_base("10", 36) == 36

# leading zeros ignored
assert basen.from_base("00ff", 16) == 255
assert basen.from_base("-007", 8) == -7
assert basen.from_base("0000", 10) == 0
assert basen.from_base("-0", 10) == 0
assert basen.from_base("+0", 10) == 0

# surrounding whitespace stripped
assert basen.from_base("  ff  ", 16) == 255
assert basen.from_base("\t-101\n", 2) == -5

# ---- from_base errors (ValueError) ----
bad_strings = [
    ("", 16),          # empty
    ("   ", 16),       # whitespace only
    ("-", 16),         # lone sign
    ("+", 16),         # lone sign
    ("g", 16),         # digit char out of base range (g=16 >= 16)
    ("2", 2),          # digit value >= base
    ("12", 2),         # contains invalid digit
    ("f f", 16),       # internal whitespace
    ("1 0", 10),       # internal whitespace
    ("--1", 10),       # double sign
    ("+-1", 10),       # double sign
    ("0x10", 16),      # x not a digit
    ("1.0", 10),       # dot not a digit
    ("١٢٣", 10),       # non-ascii digits
    ("a", 10),         # letter beyond base 10
    ("z", 35),         # z=35 >= 35
]
for s, b in bad_strings:
    try:
        basen.from_base(s, b)
        assert False, "expected ValueError for from_base(%r, %r)" % (s, b)
    except ValueError:
        pass

# base validation in from_base -> ValueError
for bad in [1, 0, -3, 37]:
    try:
        basen.from_base("1", bad)
        assert False, "expected ValueError for base %r" % bad
    except ValueError:
        pass

# wrong types -> TypeError
for bad in [123, None, b"ff", 16.0]:
    try:
        basen.from_base(bad, 16)
        assert False, "expected TypeError for s %r" % (bad,)
    except TypeError:
        pass

for bad in [16.0, "16", None, True, False]:
    try:
        basen.from_base("1", bad)
        assert False, "expected TypeError for base %r" % (bad,)
    except TypeError:
        pass

# ---- round-trip across all bases and many values ----
values = [0, 1, -1, 2, -2, 7, 8, 35, 36, 255, 256, -255, 1023, 1024,
          999999, -999999, 2**40, -(2**40), 123456789, -123456789]
for base in range(2, 37):
    for v in values:
        s = basen.to_base(v, base)
        # to_base output must round-trip and match builtin int() parsing too
        assert basen.from_base(s, base) == v, (v, base, s)
        # cross-check against Python's int() for non-negative-friendly check
        assert int(s, base) == v, (v, base, s)
        # no leading-zero garbage except lone "0"
        body = s[1:] if s[0] == "-" else s
        assert body == "0" or not body.startswith("0"), (v, base, s)

# round-trip the other direction: parse builtin -> our to_base matches our parse
for base in range(2, 37):
    for v in [0, 5, 17, 4095, 60466175, -60466175]:
        builtin = format(v if v >= 0 else -v, "x")  # just a value source
    # parse a known string and re-emit
    assert basen.to_base(basen.from_base("100", base), base) == "100"
    assert basen.to_base(basen.from_base("-100", base), base) == "-100"

print("ok")
