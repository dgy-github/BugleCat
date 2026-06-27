from expr import evaluate

assert evaluate("7") == 7
assert evaluate("2+3*4") == 14
assert evaluate("(2+3)*4") == 20
assert evaluate("10/4") == 2.5
assert evaluate(" 2 * (3 + 4) - 5 ") == 9
assert evaluate("2*3+4*5") == 26
assert evaluate("((1+2)*(3+4))") == 21
assert evaluate("1+2-3+4") == 4
assert evaluate("100/10/5") == 2.0          # left-associative
assert evaluate("2+2*2+2") == 8
assert evaluate("(1+(2+(3+4)))") == 10

print("ok")
