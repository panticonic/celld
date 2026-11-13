"""Scenario framework: bring up a fleet, run load, inject a fault, recover,
verify, write one JSON result.

Synchronisation rules (the "no blind sleeps" contract):
  * `ctx.load(seconds)` sleeps, but only to let the workload run for a fixed
    *duration* inside a phase. It never stands in for "wait until X happened".
  * Anything that depends on the cluster reaching a state (takeover done, node
    healthy, node exited, ensemble open, recovery finished) uses `wait_until`
    with a named condition and a bound derived from celld's documented
    constants; exceeding it is reported (WaitTimeout), not hung on.
"""

from __future__ import annotations

import gzip
import hashlib
import json
import os
import re
import shutil
import statistics
import time
import traceback
from pathlib import Path

from . import checker
from .cluster import Cluster, Node
from .util import WaitTimeout, now, wait_until, wall
from .workload import Workload

RECOVER_DEADLINE = float(os.environ.get("FLEET_RECOVER_DEADLINE", "180"))

HIGHLIGHT = re.compile(
    r"SELF-FENCE|ownership record|output gate|fenc|takeover|recover|sealed|"
    r"lease|posture|ensemble|handoff|shutdown|panic|ERROR|WARN",
    re.I,
)


class Ctx:
    def __init__(self, name, cluster: Cluster, wl: Workload, opts: dict):
        self.name = name
        self.cluster = cluster
        self.wl = wl
        self.opts = opts
        self.events: list[dict] = []
        self.effects: list[dict] = []
        self.data: dict = {}
        self.liveness_failures: list[str] = []
        self.ttl_s = cluster.ttl_ms / 1000.0

    # ------------------------------------------------------------ logging
    def event(self, label: str, **kw):
        t = now()
        self.events.append({"t": round(t, 3), "at": wall(t), "event": label, **kw})
        extra = " ".join(f"{k}={v}" for k, v in kw.items())
        print(f"  [{wall(t)}] {self.name}: {label} {extra}", flush=True)

    def phase(self, label: str, **kw):
        self.event(label, phase=True, **kw)

    def effect(self, name: str, ok: bool, detail: str = "", excuses: bool = False):
        """A precondition/effectiveness check: the fault actually did what the
        scenario intends. A false effect makes the verdict INCONCLUSIVE (a PASS
        proves nothing if the fault never bit)."""
        # excuses=True marks a *tolerance* precondition: if it did not hold, the
        # injected fault exceeded what celld promises to survive, so a resulting
        # violation is reported (kept in `violations`) but the verdict is
        # INCONCLUSIVE rather than FAIL.
        self.effects.append({"name": name, "ok": bool(ok), "detail": detail, "excuses": excuses})
        self.event(f"effect {'ok' if ok else 'NOT-OBSERVED'}: {name}", detail=detail)

    def liveness(self, what: str):
        self.liveness_failures.append(what)
        self.event("LIVENESS FAILURE", detail=what)

    # ------------------------------------------------------------ helpers
    def load(self, secs: float, label: str | None = None):
        """Let the workload run for `secs` (a duration, not a sync point)."""
        if label:
            self.phase(label)
        time.sleep(secs)

    def wait_acks(self, per_cell: int, timeout: float = 90):
        """Wait until every cell has `per_cell` acknowledged writes."""
        wait_until(
            lambda: all(len(self.wl.acked(c)) >= per_cell for c in self.wl.cells),
            timeout,
            f"every cell to reach {per_cell} acked writes",
        )

    def owners(self) -> dict[str, str | None]:
        return self.cluster.owner_map(self.wl.dids)

    def victim(self, most=True) -> Node:
        """The node owning the most (or fewest) cells."""
        om = self.owners()
        count = {n.name: 0 for n in self.cluster.nodes}
        for owner in om.values():
            if owner:
                count[owner] += 1
        self.data["owners_before_fault"] = om
        order = sorted(self.cluster.nodes, key=lambda n: (-count[n.name], n.name))
        v = order[0] if most else order[-1]
        self.data["victim"] = v.name
        self.data["victim_cells"] = [c for c, o in om.items() if o == v.name]
        return v

    def victim_cells(self) -> list[str]:
        return self.data["victim_cells"]

    def last_inst_before(self, cell: str, t: float):
        a = [w for w in self.wl.writes if w["outcome"] == "ack" and w["cell"] == cell and w["t1"] <= t]
        return max(a, key=lambda w: w["t1"])["inst"] if a else None

    def acked_after(self, cell: str, t: float, not_inst: str | None = None, exclude_nodes=()):
        return [
            w for w in self.wl.writes
            if w["outcome"] == "ack" and w["cell"] == cell and w["t1"] > t
            and (not_inst is None or w["inst"] != not_inst) and w["node"] not in exclude_nodes
        ]

    def wait_resume(self, cells, since: float, timeout: float, new_instance=False,
                    exclude_nodes=(), what="acks"):
        """Wait until every cell has a fresh acknowledged write issued after
        `since` (optionally by a different DO instance = a takeover happened,
        and not entered via `exclude_nodes`). Returns {cell: seconds}."""
        prior = {c: self.last_inst_before(c, since) for c in cells}
        out: dict[str, float] = {}

        def done():
            for c in cells:
                if c in out:
                    continue
                a = self.acked_after(c, since, prior[c] if new_instance else None, exclude_nodes)
                if a:
                    out[c] = round(min(w["t1"] for w in a) - since, 3)
            return len(out) == len(cells)

        wait_until(done, timeout, f"{what} on cells {cells} after t={since:.1f}", poll=0.2)
        return out

    def supervisor_restart(self, node: Node, wipe: bool = False, wait_lease: bool = True):
        """What a supervisor must do (docs/guarantees.md, "The supervisor"):
        wait at least one lease lifetime after the process died, then restart
        with the same node identity. Waits on the container's recorded finish
        time, not on a sleep."""
        if node.status() == "running":
            return
        if wait_lease:
            st = node.inspect()
            fin = st.get("FinishedAt", "")
            # Docker timestamps are RFC3339 with ns; compare against wall now.
            try:
                from datetime import datetime, timezone
                ts = datetime.fromisoformat(re.sub(r"(\.\d{6})\d*Z$", r"\1+00:00", fin))
                age = (datetime.now(timezone.utc) - ts).total_seconds()
            except Exception:
                age = 0.0
            need = self.ttl_s + 1.0 - age
            if need > 0:
                wait_until(
                    lambda: (datetime.now(timezone.utc) - ts).total_seconds() >= self.ttl_s + 1.0,
                    need + 5, f"one lease lifetime since {node.name} stopped",
                )
        self.event("supervisor restart", node=node.name, wipe=wipe)
        node.start(wipe=wipe)
        node.wait_healthy(RECOVER_DEADLINE)
        self.event("node healthy", node=node.name)

    def ensemble_open(self, node: Node, timeout=30):
        try:
            wait_until(lambda: node.logs_contain("fleet acks enabled"), timeout,
                       f"{node.name} to open its log ensemble (fleet acks)")
            return True
        except WaitTimeout:
            return False


# ---------------------------------------------------------------------- final
def recover_all(ctx: Ctx):
    """Undo injected faults, restart anything that is down (as a supervisor
    would) and wait for every node to be healthy."""
    for n in ctx.cluster.nodes:
        if n.status() == "running":
            try:
                if n.proc_state().startswith("T"):
                    n.resume()
            except Exception:
                pass
            try:
                n.heal()
                n.clear_netem()
            except Exception:
                pass
    for n in ctx.cluster.nodes:
        if n.status() != "running":
            ctx.supervisor_restart(n)
    for n in ctx.cluster.nodes:
        n.wait_healthy(RECOVER_DEADLINE)


def final_verification(ctx: Ctx) -> dict:
    wl, cl = ctx.wl, ctx.cluster
    ctx.phase("verify: recover + probe writes + read back from every node")
    recover_all(ctx)
    final: dict = {}
    recovery_s: dict = {}
    for cell in wl.cells:
        t_start = now()
        got = 0
        attempt = 0
        deadline = time.monotonic() + RECOVER_DEADLINE
        first = None
        while got < 3 and time.monotonic() < deadline:
            node = cl.nodes[attempt % len(cl.nodes)]
            rec = wl.do_write(node, cell, f"{wl.tag}-verify-{cell}-{attempt}", f"{wl.tag}-verify", timeout=30)
            attempt += 1
            wl.writes.append(rec)
            if rec["outcome"] == "ack":
                got += 1
                first = first or now() - t_start
            else:
                time.sleep(0.5)  # retry pacing inside a bounded loop
        recovery_s[cell] = round(first, 3) if first is not None else None
        if got < 3:
            ctx.liveness(f"cell {cell}: no acknowledged write within {RECOVER_DEADLINE:.0f}s after recovery")
        per_node, rows, err = {}, None, None
        for n in cl.nodes:
            node_rows = None
            dl = time.monotonic() + (RECOVER_DEADLINE if rows is None else 60)
            while time.monotonic() < dl:
                outcome, status, body, e = wl._request(n.ip, "GET", f"/dump/{cell}", timeout=30)
                if outcome == "ok":
                    node_rows = body["rows"]
                    break
                err = f"{n.name}: {outcome} {e}"
                time.sleep(0.5)
            if node_rows is None:
                per_node[n.name] = f"ERR {err}"
            else:
                per_node[n.name] = hashlib.sha1(json.dumps(node_rows).encode()).hexdigest()[:12]
                rows = rows or node_rows
        final[cell] = {"rows": rows, "per_node": per_node, "error": err if rows is None else None}
    ctx.data["recovery_s"] = recovery_s
    return final


# ---------------------------------------------------------------------- stats
def pctl(xs, p):
    if not xs:
        return None
    xs = sorted(xs)
    return round(xs[min(len(xs) - 1, int(len(xs) * p))], 4)


def phase_stats(ctx: Ctx):
    marks = [(e["t"], e["event"]) for e in ctx.events if e.get("phase")]
    out = []
    for i, (t, label) in enumerate(marks):
        t_end = marks[i + 1][0] if i + 1 < len(marks) else now()
        w = [x for x in ctx.wl.writes if t <= x["t0"] < t_end]
        ack = [x for x in w if x["outcome"] == "ack"]
        lat = [x["t1"] - x["t0"] for x in ack]
        fails = {}
        for x in w:
            if x["outcome"] != "ack":
                fails[x["outcome"]] = fails.get(x["outcome"], 0) + 1
        out.append({
            "phase": label, "t_start": t, "t_end": round(t_end, 3), "secs": round(t_end - t, 2),
            "issued": len(w), "acked": len(ack), "failed": fails,
            "acks_per_s": round(len(ack) / max(t_end - t, 1e-9), 1),
            "lat_p50_s": pctl(lat, 0.5), "lat_p99_s": pctl(lat, 0.99), "lat_max_s": pctl(lat, 1.0),
        })
    return out


def log_highlights(cl: Cluster, limit=60):
    res = {}
    for n in cl.nodes:
        try:
            lines = [ln for ln in n.logs().splitlines() if HIGHLIGHT.search(ln)]
        except Exception:
            lines = []
        keep = lines if len(lines) <= limit else lines[: limit // 2] + ["... (%d lines elided) ..." % (len(lines) - limit)] + lines[-limit // 2:]
        res[n.name] = [ln[:400] for ln in keep]
    return res


# ---------------------------------------------------------------------- driver
def run_scenario(name: str, fn, description: str, opts: dict, outdir: Path, run_id: str,
                 bin_path: str, workbase: Path) -> dict:
    started = time.time()
    t_begin = time.monotonic()
    outdir.mkdir(parents=True, exist_ok=True)
    result = {
        "scenario": name, "description": description, "run_id": run_id,
        "started_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime(started)),
        "celld_bin": os.path.realpath(bin_path), "verdict": "ERROR",
    }
    wd = workbase / f"{run_id}-{name}"
    ttl_ms = int(opts.get("ttl_ms", os.environ.get("FLEET_TTL_MS", 10000)))
    cells = [f"cell{i}" for i in range(int(opts.get("cells", 6)))]
    logdir = None
    try:
        with Cluster(f"{run_id}-{name}"[:40].replace("_", "-"), wd, bin_path, nodes=3, ttl_ms=ttl_ms,
                     rust_log=os.environ.get("FLEET_RUST_LOG", "warn,celld::node_log=info"),
                     extra_env=opts.get("env", {})) as cl:
            logdir = cl.logdir
            wl = Workload(
                cl, cells, clients=int(opts.get("clients", 12)), readers=int(opts.get("readers", 3)),
                pad=int(opts.get("pad", 0)), req_timeout=float(opts.get("req_timeout", 15)),
                think=float(opts.get("think", 0.01)), seed=int(opts.get("seed", 1)), tag="w",
            )
            ctx = Ctx(name, cl, wl, opts)
            result["config"] = {"nodes": 3, "ttl_ms": ttl_ms, "cells": len(cells), "clients": wl.n_clients,
                                "readers": wl.n_readers, "req_timeout_s": wl.req_timeout,
                                "subnet": None, "minio_image": os.environ.get("MINIO_IMAGE", "pgsty/minio:latest")}
            err = None
            final = None
            try:
                ctx.phase("bring-up")
                cl.up()
                result["config"]["subnet"] = cl.subnet + ".0/24"
                result["deploy"] = cl.deploy_output
                result["diagnose"] = cl.diagnose().strip().splitlines()[-6:]
                ctx.phase("warmup")
                wl.warmup()
                ctx.data["dids"] = dict(wl.dids)
                wl.start()
                ctx.wait_acks(15)
                ctx.phase("steady")
                try:
                    fn(ctx)
                except WaitTimeout as e:
                    ctx.liveness(str(e))
                ctx.phase("quiesce")
                wl.stop()
                final = final_verification(ctx)
            except WaitTimeout as e:
                ctx.liveness(str(e))
            except Exception:
                err = traceback.format_exc()
                ctx.event("HARNESS ERROR", detail=err.splitlines()[-1])
            finally:
                try:
                    wl.stop()
                except Exception:
                    pass
            if final is None and err is None:
                try:
                    final = final_verification(ctx)
                except Exception:
                    err = traceback.format_exc()
            chk = None
            if final:
                try:
                    chk = checker.check(wl.writes, wl.reads, final)
                except Exception:
                    err = (err or "") + traceback.format_exc()
            violations = (chk["violations"] if chk else [])
            liveness = ctx.liveness_failures
            bad_effects = [e for e in ctx.effects if not e["ok"]]
            if err:
                verdict = "ERROR"
            elif (violations or liveness) and any(e.get("excuses") for e in bad_effects):
                verdict = "INCONCLUSIVE"
            elif violations or liveness:
                verdict = "FAIL"
            elif bad_effects:
                verdict = "INCONCLUSIVE"
            else:
                verdict = "PASS"
            result.update(
                verdict=verdict,
                duration_s=round(time.monotonic() - t_begin, 1),
                invariants={
                    "acked_write_durable": not any(v["type"] in ("lost_ack",) for v in violations),
                    "single_writer": not any(v["type"] in ("duplicate_ack_seq", "order_violation", "stale_read",
                                                           "non_monotonic_read", "ledger_gap", "replica_disagreement")
                                             for v in violations),
                    "liveness_after_recovery": not liveness,
                },
                violations=violations[:60],
                liveness_failures=liveness,
                effects=ctx.effects,
                checker=chk,
                phases=phase_stats(ctx),
                timeline=ctx.events,
                data=ctx.data,
                node_exit_codes={n.name: (n.inspect().get("ExitCode"), n.inspect().get("Status")) for n in cl.nodes},
                log_highlights=log_highlights(cl),
                error=err,
            )
            # persist the raw client log and final ledgers (the evidence behind every verdict)
            if final:
                with gzip.open(outdir / f"{name}.final.json.gz", "wt") as f:
                    json.dump(final, f)
            with gzip.open(outdir / f"{name}.ops.jsonl.gz", "wt") as f:
                for r in wl.writes + wl.reads:
                    f.write(json.dumps(r) + "\n")
            (outdir / f"{name}.json").write_text(json.dumps(result, indent=1, default=str))
    except BaseException as e:  # bring-up failure etc.: still emit a result
        if result.get("verdict") == "ERROR" and "error" not in result:
            result["error"] = traceback.format_exc()
            result["duration_s"] = round(time.monotonic() - t_begin, 1)
            (outdir / f"{name}.json").write_text(json.dumps(result, indent=1, default=str))
        if isinstance(e, KeyboardInterrupt):
            raise
    finally:
        if logdir and logdir.exists():
            dst = outdir / f"{name}.logs"
            shutil.rmtree(dst, ignore_errors=True)
            try:
                shutil.copytree(logdir, dst)
            except Exception:
                pass
        shutil.rmtree(wd, ignore_errors=True)
    return result
