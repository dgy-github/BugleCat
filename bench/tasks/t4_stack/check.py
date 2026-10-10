from stack import Stack

s = Stack()
assert s.is_empty() and s.size() == 0
s.push(1)
s.push(2)
assert s.size() == 2 and s.peek() == 2
assert s.pop() == 2 and s.pop() == 1 and s.is_empty()
try:
    s.pop()
    raise SystemExit('no IndexError on empty pop')
except IndexError:
    pass

# Deeper stack: peek/size must track the top at every depth, peek must not
# remove, and falsy values (0, None, "") are ordinary items.
t = Stack()
items = [10, 0, None, "", "x", 5]
for depth, item in enumerate(items, start=1):
    t.push(item)
    assert t.size() == depth, f"size {t.size()} != {depth}"
    assert t.peek() == item, f"peek {t.peek()!r} != {item!r}"
    assert t.size() == depth, "peek must not remove"
    assert not t.is_empty()
for depth in range(len(items), 0, -1):
    assert t.pop() == items[depth - 1]
    assert t.size() == depth - 1
assert t.is_empty()

# Interleaved push/pop.
u = Stack()
u.push("a")
u.push("b")
assert u.pop() == "b"
u.push("c")
u.push("d")
assert u.peek() == "d" and u.size() == 3
assert [u.pop(), u.pop(), u.pop()] == ["d", "c", "a"]
try:
    u.pop()
    raise SystemExit('no IndexError on empty pop after reuse')
except IndexError:
    pass
print('ok')
