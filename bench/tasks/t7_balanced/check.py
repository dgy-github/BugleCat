from buggy import is_balanced

assert is_balanced("([]{})") is True
assert is_balanced("([)]") is False
assert is_balanced("(((") is False
assert is_balanced(")(") is False
assert is_balanced("a(b)c") is True
assert is_balanced("") is True
assert is_balanced("{[()()]}") is True
assert is_balanced("{[(])}") is False
assert is_balanced("]") is False
print("ok")
