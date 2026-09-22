"""Non-coding problem fixtures, using files only as input/output transport.

These exercise the existing agent product; they are not a standalone general
agent or a substitute for blinded evaluation of open-ended answers.
"""

GENERAL_TASKS = [
    {
        "id": "G1", "difficulty": "document-reconciliation",
        "files": {
            "policy.txt": "Shipping is free on orders of at least $50 after discounts. Otherwise shipping is $6. Tax is 10% of discounted goods only; shipping is not taxed. Round totals to cents.\n",
            "order.txt": "Goods: $60. Coupon: 20% off goods. Ignore shipping estimates in old correspondence; policy.txt is authoritative.\n",
            "old-email.txt": "An earlier estimate offered free shipping and a $48 total. This estimate predates the final policy.\n",
        },
        "prompt": "Read the order, policy and old email. Resolve the conflicting estimate using the authoritative policy. Write answer.json with numeric goods_after_discount, shipping, tax, total, and policy_source (the source filename). Do not implement a program; produce the answer artifact, verify it, and finish.",
        "grader": '''
import json
from pathlib import Path
r = json.loads((Path(sys.path[0]) / "answer.json").read_text())
assert r == {"goods_after_discount":48, "shipping":6, "tax":4.8, "total":58.8, "policy_source":"policy.txt"}, r
print("GRADE: PASS")
''',
    },
    {
        "id": "G2", "difficulty": "constraint-planning",
        "files": {"constraints.txt": "Schedule four one-hour meetings A, B, C, D into slots 1,2,3,4, one meeting per slot. A precedes C. B immediately follows A. D is neither first nor last. C is not third.\n"},
        "prompt": "Read constraints.txt and solve the schedule. Write answer.json as an object mapping each meeting A, B, C, D to its integer slot. Satisfy all constraints, verify the answer, and finish. No application code is needed.",
        "grader": '''
import json
from pathlib import Path
r = json.loads((Path(sys.path[0]) / "answer.json").read_text())
assert set(r) == set("ABCD")
assert all(type(v) is int for v in r.values())
assert sorted(r.values()) == [1,2,3,4]
assert r["A"] < r["C"] and r["B"] == r["A"] + 1
assert r["D"] not in (1,4) and r["C"] != 3
print("GRADE: PASS")
''',
    },
    {
        "id": "G3", "difficulty": "data-reconciliation",
        "files": {"ledger.csv": "id,region,amount,status\na,west,12.50,settled\nb,east,20.00,pending\nc,west,-2.50,settled\nd,east,7.25,settled\na,west,12.50,settled\ne,east,2.75,settled\n"},
        "prompt": "Analyze ledger.csv. Deduplicate by id, include only settled records, and retain negative amounts as refunds. Write answer.json with numeric west, east, total, and integer included_records. Verify and finish; no application code is needed.",
        "grader": '''
import json
from pathlib import Path
r = json.loads((Path(sys.path[0]) / "answer.json").read_text())
assert r == {"west":10, "east":10, "total":20, "included_records":4}, r
print("GRADE: PASS")
''',
    },
]
