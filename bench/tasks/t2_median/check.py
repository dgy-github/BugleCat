import buggy

assert buggy.median([1, 2, 3, 4]) == 2.5
assert buggy.median([1, 2, 3]) == 2
assert buggy.median([5]) == 5
# 未排序输入必须先排序再取中位（原种子缺陷：去掉排序后此处失败）
assert buggy.median([3, 1, 2]) == 2
assert buggy.median([4, 1, 3, 2]) == 2.5
print("ok")
