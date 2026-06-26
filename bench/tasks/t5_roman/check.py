import roman

cases = {1: "I", 4: "IV", 9: "IX", 14: "XIV", 40: "XL", 90: "XC",
         400: "CD", 900: "CM", 1994: "MCMXCIV", 2023: "MMXXIII", 3999: "MMMCMXCIX"}
for n, s in cases.items():
    assert roman.to_roman(n) == s, f"to_roman({n}) == {roman.to_roman(n)!r}, want {s!r}"
    assert roman.from_roman(s) == n, f"from_roman({s!r}) == {roman.from_roman(s)}, want {n}"

# Full round-trip over the whole supported range.
for n in range(1, 4000):
    assert roman.from_roman(roman.to_roman(n)) == n, f"round-trip failed at {n}"

print("ok")
