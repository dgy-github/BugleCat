from wordfreq import top_n

assert top_n("The cat the CAT a dog.", 2) == [("cat", 2), ("the", 2)]
# Tie on count -> alphabetical order.
assert top_n("b a c a b c", 3) == [("a", 2), ("b", 2), ("c", 2)]
# Tokenization on punctuation/digits; case-folding.
assert top_n("Hello, hello! WORLD world world", 1) == [("world", 3)]
assert top_n("a1 a1 b2", 2) == [("a1", 2), ("b2", 1)]
# Degenerate inputs.
assert top_n("", 5) == []
assert top_n("anything", 0) == []
assert top_n("...!!!", 3) == []
# n larger than distinct word count.
assert top_n("one two two", 10) == [("two", 2), ("one", 1)]
print("ok")
