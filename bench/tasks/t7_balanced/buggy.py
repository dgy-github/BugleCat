def is_balanced(s):
    """Return True if all (), [], {} in s are correctly matched and nested."""
    stack = []
    pairs = {")": "(", "]": "[", "}": "{"}
    for ch in s:
        if ch in "([{":
            stack.append(ch)
        elif ch in ")]}":
            # BUG: pops without checking the bracket type matches, and
            # doesn't handle a closing bracket when the stack is empty.
            stack.pop()
    return True
