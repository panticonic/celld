"""Invariant checker over the client log and the final ledger dumps.

Invariants (all of these are violations => verdict FAIL):

  lost_ack              an acknowledged write is absent from the final ledger
                        (RPO=0 broken). `kind` distinguishes "missing" (seq
                        beyond the head / hole) from "overwritten" (another op
                        holds that seq: two writers forked the cell).
  duplicate_ack_seq     two different acknowledged ops got the same seq in one
                        cell (split brain: two owners handed out the same slot).
  ledger_gap            the final ledger is not 1..N contiguous.
  order_violation       real-time order broken: write B was *issued* after write
                        A was *acknowledged* but got a lower/equal seq.
  stale_read            a read issued after an acknowledgement of write A
                        returned a head below A.seq (stale or forked owner
                        answered; includes reads served by a zombie).
  non_monotonic_read    a read issued after another read completed returned a
                        smaller head.
  replica_disagreement  nodes disagree on the ledger when asked after recovery.
  final_unreadable      a cell could not be read back (liveness, bounded wait).

Real-time claims use the single client-process monotonic clock, so they do not
depend on container clocks.
"""

from __future__ import annotations

import bisect
from collections import Counter, defaultdict

MAX_EVIDENCE = 12  # examples kept per violation type per cell


def _sweep_max_before(events, queries):
    """events: [(t1, value, tag)] sorted by t1; queries: [(t0, ...)] sorted by t0.
    Yields (query, best) where best=(value, tag) is the max value among events
    with t1 < query.t0 (strictly completed before the query started)."""
    i, best = 0, None
    for q in queries:
        while i < len(events) and events[i][0] < q[0]:
            if best is None or events[i][1] > best[0]:
                best = (events[i][1], events[i][2])
            i += 1
        yield q, best


def check(writes, reads, final) -> dict:
    """`final` = {cell: {"rows": [[seq, op, inst, ts]...] | None,
                         "per_node": {node: sha | error-str},
                         "error": str | None}}"""
    viol: list[dict] = []
    notes: list[dict] = []
    per_cell = {}

    cells = sorted(set(w["cell"] for w in writes) | set(final))
    for cell in cells:
        cw = [w for w in writes if w["cell"] == cell]
        acks = [w for w in cw if w["outcome"] == "ack"]
        cr = [r for r in reads if r["cell"] == cell and r["outcome"] == "ok"]
        f = final.get(cell) or {}
        counts = {"v": Counter()}

        def add(vtype, **kw):
            counts["v"][vtype] += 1
            if counts["v"][vtype] <= MAX_EVIDENCE:
                viol.append({"type": vtype, "cell": cell, **kw})

        rows = f.get("rows")
        if rows is None:
            add("final_unreadable", detail=f.get("error"))
            per_cell[cell] = {"acks": len(acks), "final_rows": None}
            continue
        by_seq = {r[0]: r for r in rows}
        by_op = {r[1]: r for r in rows}

        # ---- ledger shape
        seqs = [r[0] for r in rows]
        if seqs != list(range(1, len(seqs) + 1)):
            missing = sorted(set(range(1, (max(seqs) if seqs else 0) + 1)) - set(seqs))
            add("ledger_gap", missing_seqs=missing[:50], n_rows=len(seqs), max_seq=max(seqs) if seqs else 0)

        # ---- durability + fork
        for a in acks:
            row = by_seq.get(a["seq"])
            if row is not None and row[1] == a["op"]:
                continue
            if row is None:
                kind, held = "missing", None
                if a["op"] in by_op:
                    kind, held = "moved", {"seq": by_op[a["op"]][0]}
            else:
                kind, held = "overwritten", {"op": row[1], "inst": row[2]}
            add(
                "lost_ack", kind=kind, op=a["op"], seq=a["seq"], inst=a.get("inst"),
                entry_node=a["node"], acked_at=round(a["t1"], 3), ledger_has=held,
                ledger_head=len(seqs),
            )
        seq_ops = defaultdict(set)
        for a in acks:
            seq_ops[a["seq"]].add((a["op"], a.get("inst"), a["node"]))
        for seq, ops in sorted(seq_ops.items()):
            if len({o[0] for o in ops}) > 1:
                add("duplicate_ack_seq", seq=seq, acks=sorted(map(list, ops)))

        # ---- real-time order of acknowledged writes
        ev = sorted(((a["t1"], a["seq"], a) for a in acks), key=lambda x: x[0])
        qs = sorted(((a["t0"], a) for a in acks), key=lambda x: x[0])
        for (t0, b), best in _sweep_max_before(ev, qs):
            if best is not None and b["seq"] <= best[0]:
                add(
                    "order_violation", later_op=b["op"], later_seq=b["seq"], later_inst=b.get("inst"),
                    later_issued_at=round(t0, 3), earlier_op=best[1]["op"], earlier_seq=best[0],
                    earlier_inst=best[1].get("inst"), earlier_acked_at=round(best[1]["t1"], 3),
                )

        # ---- read-your-writes / no stale reads
        rq = sorted(((r["t0"], r) for r in cr), key=lambda x: x[0])
        for (t0, r), best in _sweep_max_before(ev, rq):
            if best is not None and r["head"] < best[0]:
                add(
                    "stale_read", read_node=r["node"], read_head=r["head"], read_inst=r.get("inst"),
                    read_issued_at=round(t0, 3), acked_op=best[1]["op"], acked_seq=best[0],
                    acked_inst=best[1].get("inst"), acked_at=round(best[1]["t1"], 3),
                )
        rev = sorted(((r["t1"], r["head"], r) for r in cr), key=lambda x: x[0])
        for (t0, r), best in _sweep_max_before(rev, rq):
            if best is not None and r["head"] < best[0]:
                add(
                    "non_monotonic_read", head=r["head"], earlier_head=best[0],
                    read_node=r["node"], issued_at=round(t0, 3), earlier_node=best[1]["node"],
                )

        # ---- replicas agree after recovery
        pn = f.get("per_node", {})
        shas = {v for v in pn.values() if not str(v).startswith("ERR")}
        if len(shas) > 1:
            add("replica_disagreement", per_node=pn)

        # ---- informational
        unacked = [w for w in cw if w["outcome"] != "ack"]
        present_unacked = [w for w in unacked if w["op"] in by_op]
        insts = [r[2] for r in rows]
        runs = [k for i, k in enumerate(insts) if i == 0 or insts[i - 1] != k]
        if len(runs) != len(set(runs)):
            notes.append({"type": "inst_interleave", "cell": cell,
                          "detail": "an instance reappears after another instance's rows"})
        tl = {}
        for a in sorted(acks, key=lambda w: w["t1"]):
            e = tl.setdefault(a.get("inst"), {"first_ack_t": round(a["t1"], 2), "n": 0, "entry_nodes": set(),
                                              "min_seq": a["seq"], "max_seq": a["seq"]})
            e["last_ack_t"] = round(a["t1"], 2)
            e["n"] += 1
            e["entry_nodes"].add(a["node"])
            e["min_seq"], e["max_seq"] = min(e["min_seq"], a["seq"]), max(e["max_seq"], a["seq"])
        per_cell[cell] = {
            "ack_timeline_by_instance": {k: {**v, "entry_nodes": sorted(v["entry_nodes"])} for k, v in tl.items()},
            "acks": len(acks),
            "attempts": len(cw),
            "unacked": len(unacked),
            "unacked_but_present": len(present_unacked),
            "final_rows": len(rows),
            "final_head": max(seqs) if seqs else 0,
            "instances_in_ledger": list(dict.fromkeys(insts)),
            "ack_instances": sorted({a.get("inst") for a in acks}),
            "violation_counts": dict(counts["v"]),
        }

    totals = Counter(w["outcome"] for w in writes)
    return {
        "ok": not viol,
        "violations": viol,
        "notes": notes,
        "per_cell": per_cell,
        "write_outcomes": dict(totals),
        "reads": {"ok": sum(1 for r in reads if r["outcome"] == "ok"), "failed": sum(1 for r in reads if r["outcome"] != "ok")},
    }
