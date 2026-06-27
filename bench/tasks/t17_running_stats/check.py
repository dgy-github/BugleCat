from running_stats import RunningStats

# empty behavior
s = RunningStats()
assert len(s) == 0
assert s.total() == 0
assert s.count(5) == 0
for m in ("median", "mean"):
    try:
        getattr(s, m)()
        assert False, m
    except ValueError:
        pass

# odd count median
s = RunningStats()
for v in [5, 1, 3, 2, 4]:
    s.add(v)
assert len(s) == 5
assert s.median() == 3
assert s.total() == 15
assert s.mean() == 3.0

# even count median (average of two middles)
s = RunningStats()
for v in [4, 2, 1, 3]:
    s.add(v)
assert s.median() == 2.5

# duplicates + count + remove
s = RunningStats()
for v in [2, 2, 2, 5, 5]:
    s.add(v)
assert s.count(2) == 3
assert s.count(5) == 2
assert s.count(9) == 0
assert s.median() == 2
s.remove(2)
assert s.count(2) == 2
assert len(s) == 4
assert s.median() == (2 + 5) / 2
s.remove(2)
s.remove(2)
assert s.count(2) == 0
assert s.median() == 5

# remove missing raises KeyError
s = RunningStats()
s.add(1)
try:
    s.remove(7)
    assert False
except KeyError:
    pass

# rank
s = RunningStats()
for v in [10, 20, 20, 30]:
    s.add(v)
assert s.rank(5) == 0
assert s.rank(10) == 0
assert s.rank(20) == 1
assert s.rank(25) == 3
assert s.rank(99) == 4

# quantile: linear interpolation, q in [0,1]
s = RunningStats()
for v in [1, 2, 3, 4]:
    s.add(v)
assert s.quantile(0.0) == 1
assert s.quantile(1.0) == 4
assert s.quantile(0.5) == 2.5
assert s.median() == s.quantile(0.5)
assert abs(s.quantile(0.25) - 1.75) < 1e-12

# single element quantile
s = RunningStats()
s.add(42)
assert s.quantile(0.0) == 42
assert s.quantile(0.5) == 42
assert s.quantile(1.0) == 42

# quantile out of range
s = RunningStats()
s.add(1)
for bad in (-0.1, 1.1):
    try:
        s.quantile(bad)
        assert False
    except ValueError:
        pass

# bool rejected as not a number
s = RunningStats()
try:
    s.add(True)
    assert False
except TypeError:
    pass

# floats and exact running sum/median after mixed add/remove
s = RunningStats()
for v in [1.5, 2.5, 3.5]:
    s.add(v)
assert s.total() == 7.5
assert s.median() == 2.5
s.remove(2.5)
assert s.median() == (1.5 + 3.5) / 2
assert s.total() == 5.0

print("ok")