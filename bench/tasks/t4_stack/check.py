from stack import Stack
s=Stack()
assert s.is_empty() and s.size()==0
s.push(1); s.push(2)
assert s.size()==2 and s.peek()==2
assert s.pop()==2 and s.pop()==1 and s.is_empty()
try:
    s.pop(); raise SystemExit('no IndexError on empty pop')
except IndexError:
    pass
print('ok')
