from toposort import toposort


def valid(graph, order):
    if set(order) != set(graph):
        return False
    if len(order) != len(graph):
        return False
    pos = {n: i for i, n in enumerate(order)}
    for node, deps in graph.items():
        for d in deps:
            if pos[d] >= pos[node]:   # dependency must come strictly before
                return False
    return True


g1 = {"a": [], "b": ["a"], "c": ["a", "b"], "d": ["c"]}
assert valid(g1, toposort(g1)), "g1 order invalid"

g2 = {"x": ["y"], "y": ["z"], "z": []}
assert valid(g2, toposort(g2)), "g2 order invalid"

g3 = {"a": [], "b": [], "c": []}          # all independent
assert valid(g3, toposort(g3)), "g3 order invalid"

g4 = {"a": ["b", "c"], "b": ["d"], "c": ["d"], "d": []}   # diamond
assert valid(g4, toposort(g4)), "g4 order invalid"

# Cycle -> ValueError
cyc = {"a": ["b"], "b": ["a"]}
try:
    toposort(cyc)
    raise AssertionError("expected ValueError on a cycle")
except ValueError:
    pass

# Self-cycle -> ValueError
try:
    toposort({"a": ["a"]})
    raise AssertionError("expected ValueError on a self-cycle")
except ValueError:
    pass

print("ok")
