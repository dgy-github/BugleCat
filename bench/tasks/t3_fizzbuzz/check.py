from fizzbuzz import fizzbuzz

assert fizzbuzz(5) == ['1', '2', 'Fizz', '4', 'Buzz']
r = fizzbuzz(15)
assert r[14] == 'FizzBuzz' and r[2] == 'Fizz' and r[4] == 'Buzz'


def _expected(n):
    out = []
    for i in range(1, n + 1):
        s = ("Fizz" if i % 3 == 0 else "") + ("Buzz" if i % 5 == 0 else "")
        out.append(s or str(i))
    return out


# Full lists, including beyond the first 15 (defeats hard-coding) and n=0.
for n in (0, 1, 2, 3, 14, 15, 16, 30, 31, 100):
    got = fizzbuzz(n)
    assert got == _expected(n), f"fizzbuzz({n}) mismatch: {got!r}"
print('ok')
