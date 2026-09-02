import copy
import importlib.util
import os

path = os.path.join(os.getcwd(), "solution.py")
assert os.path.exists(path), "solution.py not found"

spec = importlib.util.spec_from_file_location("solution", path)
mod = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mod)

assert hasattr(mod, "rank_scores"), "rank_scores not defined"

inp = [("bob", 30), ("amy", 30), ("cara", 50), ("dan", 10)]
snapshot = copy.deepcopy(inp)

out = mod.rank_scores(inp)

# correctness: sorted by score desc, name asc tiebreak, 1-based rank
assert out == [
    {"name": "cara", "score": 50, "rank": 1},
    {"name": "amy", "score": 30, "rank": 2},
    {"name": "bob", "score": 30, "rank": 3},
    {"name": "dan", "score": 10, "rank": 4},
], "wrong output: %r" % (out,)

# a second, independent case
inp2 = [("z", 1), ("a", 1), ("m", 9)]
snap2 = copy.deepcopy(inp2)
out2 = mod.rank_scores(inp2)
assert out2 == [
    {"name": "m", "score": 9, "rank": 1},
    {"name": "a", "score": 1, "rank": 2},
    {"name": "z", "score": 1, "rank": 3},
], "wrong output (case 2): %r" % (out2,)

# input purity: arguments must be byte-for-byte unchanged
assert inp == snapshot, "input list was mutated: %r" % (inp,)
assert inp2 == snap2, "input list was mutated: %r" % (inp2,)

# empty input handled and not the same object returned
empty = []
res = mod.rank_scores(empty)
assert res == [], "empty input should give empty list, got %r" % (res,)
assert res is not empty, "must return a new list, not the input object"

print("ok")
