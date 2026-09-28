import json
import os
import sys
import tempfile

sys.path.insert(0, os.path.dirname(os.path.abspath("_check.py")))

from models import Order  # noqa: E402
import report  # noqa: E402
import store  # noqa: E402

o = Order(order_id="A1", quantity=2, price=3.5)
assert o.total() == 7.0

fixture = [
    {"order_id": "A1", "quantity": 2, "price": 3.5},
    {"order_id": "B2", "quantity": 1, "price": 10.0},
    {"order_id": "C3", "quantity": 4, "price": 2.5},
]
fd, path = tempfile.mkstemp(suffix=".json")
try:
    with os.fdopen(fd, "w", encoding="utf-8") as handle:
        json.dump(fixture, handle)
    orders = store.load_orders(path)
    assert len(orders) == 3
    assert all(isinstance(x, Order) for x in orders)
    assert abs(store.total_revenue(orders) - 27.0) < 1e-9
    assert store.total_revenue([]) == 0.0
    assert report.build_report(path) == "orders=3 revenue=27.00"
finally:
    os.remove(path)
print("ok")
