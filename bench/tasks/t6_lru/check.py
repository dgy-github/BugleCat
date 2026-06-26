from lru import LRUCache

c = LRUCache(2)
c.put(1, 1)
c.put(2, 2)
assert c.get(1) == 1            # 1 is now most-recently used
c.put(3, 3)                     # evicts key 2 (LRU)
assert c.get(2) == -1
c.put(4, 4)                     # evicts key 1
assert c.get(1) == -1
assert c.get(3) == 3
assert c.get(4) == 4

# Updating an existing key must refresh recency, not evict.
d = LRUCache(2)
d.put(1, 1)
d.put(2, 2)
d.put(1, 10)                    # update 1 -> most-recently used
d.put(3, 3)                     # evicts 2, not 1
assert d.get(1) == 10
assert d.get(2) == -1
assert d.get(3) == 3

# Absent key returns -1.
e = LRUCache(1)
assert e.get(99) == -1

print("ok")
