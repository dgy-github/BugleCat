from intervals import merge

assert merge([[1, 3], [2, 6], [8, 10], [15, 18]]) == [[1, 6], [8, 10], [15, 18]]
assert merge([[1, 4], [4, 5]]) == [[1, 5]]            # touching merge
assert merge([]) == []
assert merge([[1, 4]]) == [[1, 4]]
assert merge([[1, 4], [0, 4]]) == [[0, 4]]
assert merge([[1, 4], [2, 3]]) == [[1, 4]]            # fully nested
assert merge([[5, 6], [1, 2]]) == [[1, 2], [5, 6]]    # unsorted input
assert merge([[1, 2], [3, 4], [5, 6]]) == [[1, 2], [3, 4], [5, 6]]  # disjoint
assert merge([[1, 10], [2, 3], [4, 5], [6, 12]]) == [[1, 12]]

print("ok")
