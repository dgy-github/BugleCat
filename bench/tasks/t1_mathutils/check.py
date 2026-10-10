import mathutils

assert mathutils.add(2, 3) == 5
assert mathutils.add(-1, 1) == 0
assert mathutils.add(0, 0) == 0
assert mathutils.add(2.5, 0.5) == 3.0
assert mathutils.add(10**12, 1) == 10**12 + 1


def _prime(n):
    if n < 2:
        return False
    return all(n % d for d in range(2, int(n ** 0.5) + 1))


# Exhaustive range: boundaries (negative/0/1/2), perfect squares (9, 25, 49,
# 121) and hard-coding are all caught here, not just the visible examples.
for n in range(-10, 300):
    got = mathutils.is_prime(n)
    assert got is _prime(n), f"is_prime({n}) -> {got!r}"
assert mathutils.is_prime(7919) is True
assert mathutils.is_prime(7917) is False
print('ok')
