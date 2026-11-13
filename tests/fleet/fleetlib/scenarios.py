"""The fault scenarios. Each function receives a Ctx with a warm 3-node fleet,
a running workload and the standard invariant checker waiting at the end.

Naming: victim = the node owning the most cells; "follower" = a node that is
not the owner of the cell in question (with 3 nodes an owner recruits both
other nodes as followers, so every other node is a follower of every cell).
Timing constants are multiples of the lease TTL (ctx.ttl_s, default 10 s).
"""

from __future__ import annotations

import threading

import os
import random

from .scenario import Ctx, RECOVER_DEADLINE, recover_all
from .util import now, wait_until

SCENARIOS: dict[str, dict] = {}


def scenario(name, description, **opts):
    def deco(fn):
        SCENARIOS[name] = {"fn": fn, "description": description, "opts": opts}
        return fn
    return deco


def _failover_bound(ctx: Ctx) -> float:
    # lease expiry (TTL) + takeover recovery gate; generous multiple of the TTL.
    return ctx.ttl_s * 6 + 30


def _pre(ctx: Ctx, most=True):
    victim = ctx.victim(most)
    ctx.event("victim chosen", node=victim.name, cells=ctx.victim_cells(),
              owners=ctx.owners())
    ctx.effect("victim owns cells", bool(ctx.victim_cells()), f"{victim.name} owns {ctx.victim_cells()}")
    ctx.effect("victim has an open fleet ensemble", ctx.ensemble_open(victim),
               "log ensemble open; fleet acks enabled")
    return victim


# ------------------------------------------------------------------ kills
def _kill9(ctx: Ctx, wipe: bool):
    v = _pre(ctx)
    ctx.load(4, "pre-fault load")
    t = now()
    ctx.phase("FAULT kill -9 owner", node=v.name)
    v.kill9()
    ctx.effect("victim process is dead", v.status() == "exited", f"exit={v.exit_code()}")
    fo = ctx.wait_resume(ctx.victim_cells(), t, _failover_bound(ctx), new_instance=True,
                         what="takeover (new DO instance acks)")
    ctx.data["failover_s"] = fo
    ctx.effect("victim cells taken over by a new instance", True, f"failover_s={fo}")
    ctx.load(6, "post-takeover load")
    ctx.phase("restart victim" + (" with WIPED disk" if wipe else ""))
    ctx.supervisor_restart(v, wipe=wipe)
    ctx.load(8, "after rejoin")


@scenario("kill9_owner", "SIGKILL the owner of several cells mid write stream; restart it with its disk")
def kill9_owner(ctx):
    _kill9(ctx, wipe=False)


@scenario("kill9_owner_wiped", "SIGKILL the owner, replace it with an empty disk: acked tails survive only on followers/bucket")
def kill9_owner_wiped(ctx):
    _kill9(ctx, wipe=True)


def _term(ctx: Ctx, how: str):
    v = _pre(ctx)
    ctx.load(4, "pre-fault load")
    t = now()
    ctx.phase(f"FAULT {how} owner", node=v.name)
    t_sig = now()
    if how == "SIGTERM":
        v.signal("TERM")
    else:
        v.shutdown_api()
    # CELLD_SHUTDOWN_TOTAL_MS default 40 s bounds the whole stop.
    code = v.wait_exited(60)
    ctx.data["drain_s"] = round(now() - t_sig, 2)
    ctx.data["victim_exit_code"] = code
    ctx.effect("victim exited by itself", True, f"exit code {code} after {ctx.data['drain_s']}s")
    fo = ctx.wait_resume(ctx.victim_cells(), t, _failover_bound(ctx), what="cells served again")
    ctx.data["failover_s"] = fo
    ctx.load(6, "post-drain load")
    ctx.phase("restart victim")
    ctx.supervisor_restart(v, wait_lease=(code != 0))
    ctx.load(8, "after rejoin")


@scenario("sigterm_owner", "SIGTERM the owner (graceful handoff) mid write stream; restart it")
def sigterm_owner(ctx):
    _term(ctx, "SIGTERM")


@scenario("drain_api_owner", "POST /shutdown (drain with handoff) on the owner mid write stream; restart it")
def drain_api_owner(ctx):
    _term(ctx, "POST /shutdown")


# ------------------------------------------------------------------ partitions
@scenario("partition_owner_peers", "Partition the owner from its peers (bucket still reachable) for 2.5 leases")
def partition_owner_peers(ctx):
    v = _pre(ctx)
    peers = ctx.cluster.ips_of(ctx.cluster.others(v))
    ctx.load(4, "pre-fault load")
    t = now()
    ctx.phase("FAULT partition owner from peers", node=v.name)
    v.partition_from(peers)
    hold = ctx.ttl_s * 2.5
    ctx.load(hold, "partitioned (peers unreachable, bucket reachable)")
    ctx.data["victim_status_during"] = v.status()
    # Did the cells keep being acknowledged while the owner was cut off from
    # its followers? (bucket proofs are still possible, or a takeover.)
    during = [w for w in ctx.wl.writes if w["outcome"] == "ack" and t < w["t1"] < t + hold
              and w["cell"] in ctx.victim_cells()]
    ctx.data["victim_cell_acks_during_partition"] = len(during)
    ctx.data["victim_cell_ack_instances_during"] = sorted({w["inst"] for w in during})
    ctx.phase("HEAL partition")
    v.heal()
    ctx.load(10, "after heal")


@scenario("partition_owner_bucket", "Partition the owner from the bucket (peers reachable) for 2.5 leases; expect self-fence + takeover")
def partition_owner_bucket(ctx):
    v = _pre(ctx)
    ctx.wl.probe(v, ctx.victim_cells()[:1], "zombie-watch-1", timeout=60)
    t = now()
    ctx.load(4, "pre-fault load")
    t = now()
    ctx.phase("FAULT partition owner from bucket", node=v.name)
    v.partition_from([ctx.cluster.minio_ip])
    ctx.load(ctx.ttl_s * 2.5, "partitioned from bucket (peers + clients reach it)")
    ctx.data["victim_status_during"] = v.status()
    ctx.data["victim_exit_code_during"] = v.exit_code()
    ctx.effect("owner lost its lease and was taken over", bool(ctx.wait_resume(
        ctx.victim_cells(), t, _failover_bound(ctx), new_instance=True, what="takeover by another node")))
    ctx.phase("HEAL partition")
    v.heal()
    ctx.load(6, "after heal")
    if v.status() != "running":
        ctx.supervisor_restart(v)
    ctx.load(6, "after rejoin")


@scenario("pause_owner_zombie", "SIGSTOP the owner past its lease, let others take over, SIGCONT: the zombie must not acknowledge",
          req_timeout=10)
def pause_owner_zombie(ctx):
    v = _pre(ctx)
    cells = ctx.victim_cells()
    # Long-timeout writers pinned to the victim: they have requests in flight
    # when it freezes, so a zombie that wakes up and answers them is observed.
    pinned = [ctx.wl.probe(v, [c], f"pinned-{c}", timeout=90) for c in cells]
    ctx.load(4, "pre-fault load")
    t = now()
    ctx.phase("FAULT SIGSTOP owner", node=v.name)
    v.pause()
    ctx.effect("victim process is stopped", v.proc_state().startswith("T"), f"ps state {v.proc_state()!r}")
    fo = ctx.wait_resume(cells, t, _failover_bound(ctx), new_instance=True, exclude_nodes=(v.name,),
                         what="takeover while the owner is frozen")
    ctx.data["failover_s"] = fo
    ctx.load(ctx.ttl_s, "more writes on the new owner while the old one is frozen")
    t_resume = now()
    ctx.phase("FAULT SIGCONT owner (zombie wakes up)", node=v.name)
    v.resume()
    # Fresh pinned probes right at the zombie's public port, in addition to the
    # blocked in-flight ones.
    post = [ctx.wl.probe(v, [c], f"post-resume-{c}", timeout=30) for c in cells]
    ctx.load(10, "zombie probe window")
    for p in post:
        p.stop()
    zombie_acks = [w for w in ctx.wl.writes if w["outcome"] == "ack" and w["node"] == v.name
                   and w["t1"] > t_resume and w["cell"] in cells]
    ctx.data["acks_via_resumed_node_after_sigcont"] = len(zombie_acks)
    ctx.data["resumed_node_ack_instances"] = sorted({w["inst"] for w in zombie_acks})
    ctx.data["resumed_node_status"] = v.status()
    ctx.data["resumed_node_exit_code"] = v.exit_code()
    for p in pinned:
        p.stop()
    if v.status() != "running":
        ctx.supervisor_restart(v)
    ctx.load(6, "after rejoin")


# ------------------------------------------------------------------ followers
@scenario("kill_follower", "SIGKILL a node that owns few cells (a follower for the rest) mid write stream; restart it")
def kill_follower(ctx):
    v = _pre(ctx, most=False)
    others_cells = [c for c in ctx.wl.cells if c not in ctx.victim_cells()]
    ctx.load(4, "pre-fault load")
    t = now()
    ctx.phase("FAULT kill -9 follower", node=v.name)
    v.kill9()
    if ctx.victim_cells():
        ctx.data["failover_s"] = ctx.wait_resume(ctx.victim_cells(), t, _failover_bound(ctx), new_instance=True,
                                                 what="takeover of the dead node's own cells")
    ctx.load(8, "running with one follower less")
    # Cells owned by the surviving nodes must have kept acknowledging throughout.
    kept = {c: len(ctx.acked_after(c, t)) for c in others_cells}
    ctx.effect("surviving owners kept acknowledging", all(n > 0 for n in kept.values()), f"acks since kill: {kept}")
    ctx.phase("restart follower")
    ctx.supervisor_restart(v)
    ctx.load(8, "after rejoin (ensemble repair)")


@scenario("lag_follower", "Add 1.5 s +-0.3 s latency on one node's traffic to its peers for 3 leases (netem)")
def lag_follower(ctx):
    v = _pre(ctx, most=False)
    peers = ctx.cluster.ips_of(ctx.cluster.others(v))
    ctx.load(4, "pre-fault load")
    ctx.phase("FAULT netem delay on follower's peer links", node=v.name)
    v.netem(peers, 1500, 300)
    ctx.load(ctx.ttl_s * 3, "lagging follower")
    ctx.phase("HEAL netem")
    v.clear_netem()
    ctx.load(8, "after heal")


@scenario("pause_follower", "SIGSTOP a node for 2 leases (frozen follower), then SIGCONT",
          req_timeout=10)
def pause_follower(ctx):
    v = _pre(ctx, most=False)
    ctx.load(4, "pre-fault load")
    t = now()
    ctx.phase("FAULT SIGSTOP follower", node=v.name)
    v.pause()
    ctx.effect("follower process is stopped", v.proc_state().startswith("T"), f"ps state {v.proc_state()!r}")
    ctx.load(ctx.ttl_s * 2, "follower frozen")
    ctx.phase("SIGCONT follower")
    v.resume()
    ctx.load(10, "after resume")
    if v.status() != "running":
        ctx.supervisor_restart(v)


# ------------------------------------------------------------------ fleet restarts
def _par(fns):
    ts = [threading.Thread(target=f) for f in fns]
    [t.start() for t in ts]
    [t.join() for t in ts]


@scenario("restart_fleet_kill9", "SIGKILL all three nodes at once mid write stream, restart them (disks kept)")
def restart_fleet_kill9(ctx):
    _restart_fleet(ctx, kill=True, wipe=[])


@scenario("restart_fleet_kill9_wipe_one", "SIGKILL all nodes at once, restart with one node's disk wiped")
def restart_fleet_kill9_wipe_one(ctx):
    _restart_fleet(ctx, kill=True, wipe=["n1"])


@scenario("restart_fleet_graceful", "SIGTERM all nodes at once, restart them (disks kept)")
def restart_fleet_graceful(ctx):
    _restart_fleet(ctx, kill=False, wipe=[])


@scenario("restart_fleet_graceful_wiped", "SIGTERM all nodes at once, wipe EVERY disk, restart: state must come from the bucket alone")
def restart_fleet_graceful_wiped(ctx):
    _restart_fleet(ctx, kill=False, wipe=["n1", "n2", "n3"])


def _restart_fleet(ctx: Ctx, kill: bool, wipe: list[str]):
    cl = ctx.cluster
    for n in cl.nodes:
        ctx.ensemble_open(n, 10)
    ctx.load(4, "pre-fault load")
    ctx.phase("FAULT " + ("kill -9" if kill else "SIGTERM") + " every node")
    t0 = now()
    if kill:
        _par([n.kill9 for n in cl.nodes])
    else:
        _par([lambda n=n: n.signal("TERM") for n in cl.nodes])
        for n in cl.nodes:
            code = n.wait_exited(60)
            ctx.data.setdefault("exit_codes", {})[n.name] = code
        ctx.data["graceful_stop_s"] = round(now() - t0, 2)
    ctx.effect("whole fleet is down", all(n.status() == "exited" for n in cl.nodes), "")
    # Supervisor contract: one lease lifetime between stop and restart.
    wait_until(lambda: now() - t0 >= ctx.ttl_s + 1, ctx.ttl_s + 5, "one lease lifetime since the fleet stopped")
    ctx.phase("restart every node" + (f" (wiped: {wipe})" if wipe else ""))
    t_restart = now()
    _par([lambda n=n: n.start(wipe=n.name in wipe) for n in cl.nodes])
    for n in cl.nodes:
        n.wait_healthy(RECOVER_DEADLINE)
    ctx.data["fleet_healthy_after_restart_s"] = round(now() - t_restart, 2)
    ctx.wait_resume(ctx.wl.cells, t_restart, RECOVER_DEADLINE, what="every cell acknowledging again")
    ctx.load(8, "after restart")


# ------------------------------------------------------------------ slow bucket / multi-fault
@scenario("slow_bucket", "Delay every node's bucket traffic by 400 ms +-100 ms for 3 leases (netem)")
def slow_bucket(ctx):
    cl = ctx.cluster
    for n in cl.nodes:
        ctx.ensemble_open(n, 10)
    ctx.load(4, "pre-fault load")
    ctx.phase("FAULT netem delay on all nodes -> bucket")
    for n in cl.nodes:
        n.netem([cl.minio_ip], 400, 100)
    ctx.load(ctx.ttl_s * 3, "slow bucket")
    ctx.phase("HEAL netem")
    for n in cl.nodes:
        n.clear_netem()
    ctx.load(8, "after heal")


@scenario("slow_bucket_kill_owner", "Slow bucket (uploads lag behind acks), then SIGKILL the owner and replace its disk")
def slow_bucket_kill_owner(ctx):
    cl = ctx.cluster
    v = _pre(ctx)
    ctx.phase("FAULT netem delay on all nodes -> bucket (400 ms)")
    for n in cl.nodes:
        n.netem([cl.minio_ip], 400, 100)
    ctx.load(8, "slow bucket, uploads lagging")
    t = now()
    ctx.phase("FAULT kill -9 owner under slow bucket", node=v.name)
    v.kill9()
    ctx.data["failover_s"] = ctx.wait_resume(ctx.victim_cells(), t, _failover_bound(ctx) + 30, new_instance=True,
                                             what="takeover under a slow bucket")
    ctx.load(6, "post-takeover load (slow bucket)")
    ctx.phase("HEAL netem; restart victim with WIPED disk")
    for n in cl.others(v):
        n.clear_netem()
    ctx.supervisor_restart(v, wipe=True)
    ctx.load(8, "after rejoin")


@scenario("kill_two_nodes_wiped", "SIGKILL two of three nodes at once (owner + one follower), replace both disks: the survivor holds the only copy of acked tails")
def kill_two_nodes_wiped(ctx):
    cl = ctx.cluster
    v = _pre(ctx)
    other = sorted(cl.others(v), key=lambda n: n.name)[0]
    survivor = [n for n in cl.nodes if n not in (v, other)][0]
    # The claim under test needs three copies of each acked tail: the owner's
    # ensemble must contain BOTH other nodes when the fault lands. (Under load a
    # gray-follower eviction can leave an ensemble of one follower; killing and
    # wiping the owner plus its only follower would then be beyond the design's
    # fault tolerance, not a bug.)
    want = sorted(n.name for n in cl.others(v))
    try:
        wait_until(lambda: v.ensemble_members() == want, 40, f"{v.name} ensemble to hold both followers {want}")
    except Exception:
        pass
    ctx.effect("owner's ensemble holds both other nodes at fault time", v.ensemble_members() == want,
               f"members={v.ensemble_members()}", excuses=True)
    ctx.load(4, "pre-fault load")
    t = now()
    ctx.effect("owner's ensemble still holds both other nodes", v.ensemble_members() == want,
               f"members={v.ensemble_members()}", excuses=True)
    ctx.phase("FAULT kill -9 two nodes", nodes=[v.name, other.name], survivor=survivor.name)
    ctx.data["ensemble_members_at_fault"] = v.ensemble_members()
    _par([v.kill9, other.kill9])
    ctx.effect("two nodes are dead", v.status() == "exited" and other.status() == "exited", "")
    ctx.load(ctx.ttl_s * 2, "one node left")
    ctx.phase("restart both with WIPED disks")
    _par([lambda: ctx.supervisor_restart(v, wipe=True), lambda: ctx.supervisor_restart(other, wipe=True)])
    ctx.wait_resume(ctx.wl.cells, t, RECOVER_DEADLINE, what="every cell acknowledging again")
    ctx.load(8, "after rejoin")


@scenario("chaos_soak", "Seeded random single faults one after another (kill -9, wiped kill, SIGTERM, SIGSTOP, partitions, netem), each fully recovered before the next",
          req_timeout=12, rounds=6)
def chaos_soak(ctx):
    seed = int(os.environ.get("FLEET_SEED", "1"))
    rng = random.Random(seed)
    cl = ctx.cluster
    rounds = int(ctx.opts.get("rounds", 6))
    ctx.data["seed"] = seed
    ctx.data["rounds"] = []
    kinds = ["kill9", "kill9_wiped", "sigterm", "pause", "partition_bucket", "partition_peers", "netem_peers"]
    ctx.load(3, "warm load")
    for i in range(rounds):
        kind = rng.choice(kinds)
        node = rng.choice(cl.nodes)
        om = ctx.owners()
        mine = [c for c, o in om.items() if o == node.name]
        t = now()
        ctx.phase(f"round {i + 1}/{rounds}: {kind} on {node.name}", owned=mine)
        ctx.data["rounds"].append({"round": i + 1, "fault": kind, "node": node.name, "owned_cells": mine})
        hold = ctx.ttl_s * 2.5
        if kind in ("kill9", "kill9_wiped"):
            node.kill9()
            if mine:
                ctx.wait_resume(mine, t, _failover_bound(ctx), new_instance=True, what=f"takeover of {mine}")
            ctx.load(2)
            ctx.supervisor_restart(node, wipe=(kind == "kill9_wiped"))
        elif kind == "sigterm":
            node.signal("TERM")
            node.wait_exited(60)
            if mine:
                ctx.wait_resume(mine, t, _failover_bound(ctx), what=f"cells {mine} served again")
            ctx.supervisor_restart(node, wait_lease=(node.exit_code() != 0))
        elif kind == "pause":
            node.pause()
            ctx.load(hold)
            node.resume()
        elif kind == "partition_bucket":
            node.partition_from([cl.minio_ip])
            ctx.load(hold)
            node.heal()
        elif kind == "partition_peers":
            node.partition_from(cl.ips_of(cl.others(node)))
            ctx.load(hold)
            node.heal()
        elif kind == "netem_peers":
            node.netem(cl.ips_of(cl.others(node)), 1200, 300)
            ctx.load(hold)
            node.clear_netem()
        recover_all(ctx)  # heal, restart anything that exited (self-fence), wait healthy
        ctx.wait_resume(ctx.wl.cells, t, RECOVER_DEADLINE, what=f"all cells acking after round {i + 1}")
        ctx.data["rounds"][-1]["recovered_after_s"] = round(now() - t, 1)
        ctx.load(4)
