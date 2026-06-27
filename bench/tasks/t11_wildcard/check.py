from wildcard import is_match

assert is_match("", "") is True
assert is_match("", "*") is True
assert is_match("", "?") is False
assert is_match("abc", "a?c") is True
assert is_match("abc", "a*c") is True
assert is_match("abc", "*") is True
assert is_match("abc", "a*d") is False
assert is_match("ab", "a") is False
assert is_match("aa", "a") is False
assert is_match("adceb", "*a*b") is True
assert is_match("acdcb", "a*c?b") is False
assert is_match("abc", "abc") is True
assert is_match("abc", "ab?") is True
assert is_match("mississippi", "m*iss*ip*i") is True
assert is_match("xaylmz", "x?y*z") is True

print("ok")
