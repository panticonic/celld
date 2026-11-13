"""Checker self-test: feed the invariant checker synthetic client logs that
break each invariant and assert it reports exactly that violation. A checker
that cannot fail would make every PASS meaningless. Needs no docker.

  tests/fleet/run selftest
"""

from __future__ import annotations

from . import checker


def W(op, cell, seq, t0, t1, inst="A", node="n1", outcome="ack"):
    return dict(kind="w", op=op, cell=cell, seq=seq, t0=t0, t1=t1, inst=inst, node=node,
                outcome=outcome, client="c", err="")


def R(cell, head, t0, t1, node="n1", inst="A"):
    return dict(kind="r", cell=cell, head=head, count=head, t0=t0, t1=t1, node=node, inst=inst,
                outcome="ok", client="r", err="")


def F(rows, per_node=None):
    sha = "x"
    return {"rows": [[s, o, i, 0] for s, o, i in rows], "per_node": per_node or {"n1": sha, "n2": sha, "n3": sha}}


def types(res):
    return sorted({v["type"] for v in res["violations"]})


CASES = []


def case(fn):
    CASES.append(fn)
    return fn


@case
def clean():
    w = [W("a", "c1", 1, 0, 1), W("b", "c1", 2, 2, 3), W("x", "c1", None, 4, 5, outcome="timeout")]
    r = [R("c1", 1, 1.5, 1.6), R("c1", 2, 3.5, 3.6)]
    res = checker.check(w, r, {"c1": F([(1, "a", "A"), (2, "b", "A"), (3, "x", "A")])})
    assert res["ok"], res["violations"]
    assert res["per_cell"]["c1"]["unacked_but_present"] == 1


@case
def lost_ack_missing():
    w = [W("a", "c1", 1, 0, 1), W("b", "c1", 2, 2, 3)]
    res = checker.check(w, [], {"c1": F([(1, "a", "A")])})
    assert types(res) == ["lost_ack"] and res["violations"][0]["kind"] == "missing", res["violations"]


@case
def lost_ack_overwritten_is_a_fork():
    w = [W("a", "c1", 1, 0, 1), W("b", "c1", 2, 2, 3, inst="A")]
    res = checker.check(w, [], {"c1": F([(1, "a", "A"), (2, "other", "B")])})
    assert types(res) == ["lost_ack"] and res["violations"][0]["kind"] == "overwritten", res["violations"]


@case
def duplicate_ack_seq():
    w = [W("a", "c1", 1, 0, 1, inst="A"), W("b", "c1", 1, 0.5, 1.5, inst="B")]
    res = checker.check(w, [], {"c1": F([(1, "a", "A")])})
    assert "duplicate_ack_seq" in types(res) and "lost_ack" in types(res), res["violations"]


@case
def ledger_gap():
    w = [W("a", "c1", 1, 0, 1), W("c", "c1", 3, 4, 5)]
    res = checker.check(w, [], {"c1": F([(1, "a", "A"), (3, "c", "A")])})
    assert "ledger_gap" in types(res), res["violations"]


@case
def real_time_order_violation():
    # b was issued after a was acknowledged but got a smaller seq.
    w = [W("a", "c1", 2, 0, 1), W("b", "c1", 1, 2, 3)]
    res = checker.check(w, [], {"c1": F([(1, "b", "B"), (2, "a", "A")])})
    assert "order_violation" in types(res), res["violations"]


@case
def concurrent_writes_may_reorder():
    w = [W("a", "c1", 2, 0, 3), W("b", "c1", 1, 1, 2)]  # overlapping: any order is fine
    res = checker.check(w, [], {"c1": F([(1, "b", "B"), (2, "a", "A")])})
    assert res["ok"], res["violations"]


@case
def stale_read():
    w = [W("a", "c1", 5, 0, 1)] + [W(f"p{i}", "c1", i, -10 + i, -9 + i) for i in range(1, 5)]
    r = [R("c1", 3, 2, 2.1)]  # issued after ack of seq 5, saw head 3
    res = checker.check(w, r, {"c1": F([(i, f"p{i}", "A") for i in range(1, 5)] + [(5, "a", "A")])})
    assert types(res) == ["stale_read"], res["violations"]


@case
def non_monotonic_read():
    r = [R("c1", 5, 0, 1), R("c1", 4, 2, 3)]
    res = checker.check([], r, {"c1": F([(i, f"p{i}", "A") for i in range(1, 6)])})
    assert types(res) == ["non_monotonic_read"], res["violations"]


@case
def replica_disagreement():
    w = [W("a", "c1", 1, 0, 1)]
    res = checker.check(w, [], {"c1": F([(1, "a", "A")], per_node={"n1": "x", "n2": "y", "n3": "x"})})
    assert types(res) == ["replica_disagreement"], res["violations"]


@case
def unreadable_cell_is_reported():
    res = checker.check([W("a", "c1", 1, 0, 1)], [], {"c1": {"rows": None, "per_node": {}, "error": "boom"}})
    assert types(res) == ["final_unreadable"], res["violations"]


def main() -> int:
    bad = 0
    for fn in CASES:
        try:
            fn()
            print(f"  ok    {fn.__name__}")
        except AssertionError as e:
            bad += 1
            print(f"  FAIL  {fn.__name__}: {e}")
    print(f"checker selftest: {len(CASES) - bad}/{len(CASES)} passed")
    return 1 if bad else 0
