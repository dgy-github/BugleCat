from overlap import find_overlapping

# empty text / pattern longer / basic
assert find_overlapping("", "a") == []
assert find_overlapping("abc", "abcd") == []
assert find_overlapping("abc", "d") == []

# exact match
assert find_overlapping("abc", "abc") == [0]
assert find_overlapping("hello", "lo") == [3]

# overlapping ASCII
assert find_overlapping("aaa", "aa") == [0, 1]
assert find_overlapping("ABABA", "ABA") == [0, 2]
assert find_overlapping("aaaaaa", "aaaa") == [0, 1, 2]

# unicode - precomposed
assert find_overlapping("é", "é") == [0]
# combining sequence
assert find_overlapping("e\u0301", "e\u0301") == [0]
# mismatch between precomposed and decomposed
assert find_overlapping("e\u0301", "é") == []

# multi-byte emoji (single code point)
assert find_overlapping("😊😊", "😊") == [0, 1]
assert find_overlapping("😊", "😊😊") == []

# family emoji (multiple code points)
family = "👨‍👩‍👦"
assert find_overlapping(family, family) == [0]

print("ok")