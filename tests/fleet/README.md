# Fleet fault-injection harness

Reproducible checks of celld's two core promises on a real multi-node fleet:

1. an acknowledged write is durable (RPO = 0),
2. one writer per cell at a time.

It starts MinIO and three celld nodes in Docker, deploys a purpose-built
ledger Durable Object, drives concurrent clients across several cells while
injecting a fault, then checks the client-side log of acknowledgements against
what the cells actually contain afterwards. One JSON result per scenario.

Nothing here modifies celld; it runs a prebuilt binary.

## Quick start

```sh
tests/fleet/run list                     # scenarios
tests/fleet/run selftest                 # the checker must catch synthetic violations (no docker)
tests/fleet/run kill9_owner              # one scenario (~40 s)
tests/fleet/run all --jobs 2             # everything, 2 fleets in parallel
tests/fleet/run clean                    # remove containers/networks left by a crashed run
```

Requirements: Linux, Docker (no root needed beyond docker group), `python3`
(3.10+, stdlib only), `npm` or `esbuild` on `PATH` (celld's `deploy` shells out to
esbuild; the harness installs esbuild 0.25.10 into `tests/fleet/.cache/` if it
is missing). The first run builds the small `celld-fleet-node:1` image
(Ubuntu 24.04 + iptables/iproute2/curl) and pulls the MinIO image.

| env | default | meaning |
| --- | --- | --- |
| `CELLD_BIN` | `/home/werg/celld-harness-bin/celld-uf-7c7d91f` | celld binary, bind-mounted read-only into every node container. Must be a Linux x86_64 build whose glibc needs are met by Ubuntu 24.04 (2.39). |
| `FLEET_WORK` | `tests/fleet/.work` | scratch dir for node disks and MinIO data. Must be disk-backed; the harness refuses tmpfs/ramfs (`FLEET_ALLOW_TMPFS=1` overrides). |
| `FLEET_TTL_MS` / `--ttl-ms` | `10000` | `CELLD_TTL_MS` of the nodes (celld's default). |
| `MINIO_IMAGE` | `pgsty/minio:latest` | see "MinIO" below. |
| `FLEET_RUST_LOG` | `warn,celld::node_log=info` | node log filter (logs are saved per scenario). |
| `FLEET_RECOVER_DEADLINE` | `180` | seconds a cell may take to acknowledge again after recovery before it is a liveness FAIL. |
| `FLEET_KEEP=1` | off | keep scratch data after the run. |
| `FLEET_SEED` | `1` | seed for `chaos_soak`. |

Results go to `tests/fleet/results/<run id>/` (git-ignored): `<scenario>.json`,
`<scenario>.ops.jsonl.gz` (the raw client log), `<scenario>.final.json.gz` (the
ledgers read back), `<scenario>.logs/` (per-node and MinIO container logs) and
`summary.json`. Exit status is 0 only if every requested scenario is `PASS`.

## How a fleet is configured (from the source)

A fleet is every celld process that shares one bucket. There is no join step,
no peer list and no shared secret to distribute:

* **Bucket**: `--bucket s3://NAME[/PREFIX]` (or `CELLD_BUCKET`), with
  `--endpoint http://host:9000` (or `S3_ENDPOINT`) for an S3-compatible store;
  an explicit endpoint implies path-style requests and allows plain http
  (`bucket.rs`). Credentials come from the standard AWS chain
  (`AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, `AWS_REGION`).
* **Peer discovery**: through the bucket. Each node writes a lease record
  (`nodes/<node>.json`, TTL `CELLD_TTL_MS`, renewed every TTL/3) containing its
  advertised address. `--internal-listen IP:PORT` and `--advertise IP:PORT` give
  the peer/operator listener and the address peers dial (an explicit advertise
  requires an explicit internal listen address; both are private-network only).
* **Fleet HMAC secret**: not configured. The first node creates
  `fleet/peer-auth.json` in the bucket (conditional create) and every node reads
  it. Whoever can read the bucket is a fleet administrator.
* **Node identity**: `CELLD_NODE` (default: random). Reusing it on restart lets a
  node recover its own node log during startup; a new identity waits for the
  old lease to expire. The harness keeps `n1..n3` stable across restarts.
* **Deploy**: `celld deploy PROJECT --bucket ... --endpoint ...` (needs esbuild on
  PATH) writes `deploy/...` objects and the `deploy/current.json` pointer; nodes
  load the current deployment at start and poll the pointer. The harness deploys
  once before the nodes start.
* **Local state**: `CELLD_WATCH` (SQLite and replication working dir), bind-mounted
  from the host scratch dir so a "replacement machine" is `rm -rf` of that dir.
* **Durability mode**: `CELLD_DURABILITY=fleet` (default): with two or more
  nodes an acknowledgement waits for follower fsyncs (up to two followers per
  owner) or the bucket upload, whichever wins; `bucket` always waits for the bucket.
* **Health**: `GET /.well-known/celld/health` on the public listener; `GET /state`
  on the internal listener (`residents` shows which node holds a cell; the harness
  uses it for owner discovery); `POST /shutdown[?handoff=preserve]` on the
  internal listener is the drain API.
* SIGTERM begins the same graceful shutdown as `/shutdown`; a node that loses
  its lease exits with code 3 (`SELF-FENCE:`) and relies on a supervisor.

### Topology the harness builds

```
host (harness, clients)  ----bridge---+--- 10.x.y.2   MinIO  (bucket "fleet")
                                      +--- 10.x.y.11  n1: celld PID 1, :8080 public, :8081 internal
                                      +--- 10.x.y.12  n2
                                      +--- 10.x.y.13  n3
```

A fresh docker network (random /24 inside 10.150-199.0.0/16, retried on
overlap) and fresh MinIO per scenario, so scenarios cannot contaminate each other
and several can run in parallel. Clients run on the host and talk to node
containers directly over the bridge, so node-side iptables rules (which only name
peer or MinIO addresses) never cut clients off.

celld runs as **PID 1** of its container, with no init shim, so `docker kill -s
STOP|CONT|KILL|TERM` hits celld itself. The container runs as the host uid, so
cleanup needs no root. Containers have `NET_ADMIN`; faults are applied with
`docker exec --user 0`:

* partition: `iptables -I INPUT -s IP -j DROP; iptables -I OUTPUT -d IP -j DROP`
  (silent drop, both directions, in the node's own netns),
* slow link: `tc qdisc ... prio` + `netem delay D J` filtered to destination IPs
  (so "slow bucket" delays only traffic to MinIO),
* pause: SIGSTOP/SIGCONT to PID 1 (TCP stacks in the netns keep answering, as with
  a stopped process),
* kill: SIGKILL; restart is `docker start` of the same container (same identity,
  same disk, or a wiped disk to model a replacement machine).

### MinIO

`docs/guarantees.md` asks for MinIO RELEASE.2025-09-07T16-13-09Z or later.
Upstream stopped publishing those images and binaries (HTTP 410), so the default
is the community fork `pgsty/minio:latest` (reports `RELEASE.2026-08-04T00-00-00Z`
and passes `celld diagnose`'s four conditional-write probes, which each result
records). Set `MINIO_IMAGE` to use another S3-compatible image; MinIO is
documented as *not qualified for production*, so treat these as protocol-level
results, not storage-durability results (the bucket here is one MinIO on a local disk).

## The application (`app/`)

`Ledger` is a SQLite Durable Object. `POST /append/<cell>?op=<id>` runs one
synchronous SQL `INSERT` that assigns `seq = MAX(seq)+1` and returns
`{seq, op, inst, did}`; that HTTP 200 is the **acknowledgement** the output gate
holds until the write is durable. `GET /head/<cell>` is the reader and
`GET /dump/<cell>` returns the whole ledger. `inst` is a random id minted per DO
object construction, so a takeover or reactivation is visible in the log.

## Workload and checker

12 client threads write to 6 cells through a randomly chosen live node (a node
that just refused or timed out is avoided for 2 s, like a load-balancer health
check); 3 reader threads poll `/head`. Every request is logged with a unique op
id, the entry node, start/end on one monotonic clock and its outcome (`ack`,
`connect_fail`, `timeout`, `http_<code>`, `error`). Only `ack` creates an
obligation; anything else is "maybe applied".

After the scenario the harness stops the workload, heals/restarts everything,
waits (bounded) until each cell acknowledges three more writes, then dumps each
cell **from every node** and runs the checker (`fleetlib/checker.py`):

| violation | meaning |
| --- | --- |
| `lost_ack` | an acked write is not in the final ledger. `kind=overwritten`: another op occupies its seq (forked lineage); `missing`: absent. |
| `duplicate_ack_seq` | two different ops acked at the same seq (two writers) |
| `ledger_gap` | final ledger is not 1..N |
| `order_violation` | write B issued after write A was acked but got a lower seq |
| `stale_read` | a read issued after an ack returned a head below that ack's seq |
| `non_monotonic_read` | a later read returned a smaller head |
| `replica_disagreement` | nodes serve different ledgers after recovery |
| `final_unreadable` | a cell could not be read back |

Plus liveness: a cell that cannot acknowledge within `FLEET_RECOVER_DEADLINE`
after recovery is a FAIL. `tests/fleet/run selftest` feeds the checker synthetic
logs for each violation and requires it to flag them.

### Verdicts

* `PASS`: invariants held and the fault demonstrably happened (each scenario
  records "effects": the victim owned cells, the process really stopped, takeover
  was observed...).
* `FAIL`: a violation or a liveness timeout.
* `INCONCLUSIVE`: an effect was not observed (the fault did not bite), or a
  *tolerance precondition* did not hold (see `kill_two_nodes_wiped`: the owner's
  ensemble did not contain both other nodes, so wiping owner + one follower exceeds
  the three-copies tolerance). Violations are still listed in the JSON.
* `ERROR`: harness bug or bring-up failure (traceback in the JSON).

### Synchronisation (no blind sleeps)

`ctx.load(n)` sleeps only to let the workload run for a fixed *duration* in a
phase. Every correctness dependency (node healthy, node exited, takeover seen, one
lease lifetime elapsed since a stop, ensemble open, cell acking again) is a
`wait_until(condition, bound, description)`, with bounds derived from celld's
constants (lease TTL 10 s, shutdown total 40 s, readiness gate 120 s, recovery
retries). A timeout is reported as a liveness failure rather than hanging. The
supervisor contract from `docs/guarantees.md` (restart only after at least one
lease lifetime since the process stopped) is enforced from the container's
recorded finish time.

## Scenarios

| name | fault |
| --- | --- |
| `kill9_owner` | SIGKILL the node owning the most cells mid write stream; restart with its disk |
| `kill9_owner_wiped` | same, restarted with an empty disk (replacement machine) |
| `sigterm_owner` | SIGTERM the owner (graceful handoff) |
| `drain_api_owner` | `POST /shutdown` (drain with handoff) on the owner |
| `partition_owner_peers` | owner cut from both peers (bucket reachable) for 25 s |
| `partition_owner_bucket` | owner cut from the bucket (peers reachable) for 25 s; expect self-fence + takeover |
| `pause_owner_zombie` | SIGSTOP owner past its lease, takeover, SIGCONT; pinned long-timeout requests + fresh probes ask the zombie to ack |
| `kill_follower` | SIGKILL the node owning the fewest cells (a follower for the rest) |
| `lag_follower` | netem 1.5 s +-0.3 s on one node's links to its peers for 30 s |
| `pause_follower` | SIGSTOP a node for 20 s then SIGCONT |
| `restart_fleet_kill9` | SIGKILL all three nodes at once, restart (disks kept) |
| `restart_fleet_kill9_wipe_one` | same with one node's disk wiped |
| `restart_fleet_graceful` | SIGTERM all nodes at once, restart (disks kept) |
| `restart_fleet_graceful_wiped` | SIGTERM all, wipe every disk, restart: state must come from the bucket |
| `slow_bucket` | netem 400 ms +-100 ms on every node's traffic to MinIO for 30 s |
| `slow_bucket_kill_owner` | slow bucket (uploads lag behind acks), then SIGKILL the owner, replace its disk |
| `kill_two_nodes_wiped` | SIGKILL owner + one follower together, wipe both: the one survivor holds the only copy of acked tails (precondition: owner's ensemble holds both others) |
| `chaos_soak` | seeded random single faults (kill, wiped kill, SIGTERM, SIGSTOP, bucket/peer partition, netem), fully recovered between rounds |

## Limitations

* One MinIO on one disk is the bucket: it cannot model object-store tail latency,
  eventual-consistency anomalies or bucket-side data loss. "Slow bucket" is
  netem delay on each node's egress to MinIO, not throttling/429s.
* The fault domain is one host: all nodes share a kernel, a clock and one disk
  controller. No clock skew injection, no disk-full/EIO/fsync-lie faults, and a
  "kill" never loses data already in the host page cache (so SIGKILL is not a
  power-loss test).
* 3 nodes only; with 3, every other node is a follower of every owner, so
  "follower fault" scenarios fault a node that also owns cells.
* Timings depend on host load; with `--jobs > 1` CPU contention makes celld's
  gray-follower detection evict followers more often (visible in the node logs).
* Client requests are single-attempt; the harness does not model retry
  idempotency, only what the cluster acknowledged.
* The harness runs celld as shipped; log-level evidence is limited to
  `warn,celld::node_log=info` by default.

## Baseline results

`tests/fleet/baseline/` holds the per-scenario JSON (verdict, effects, timeline,
phase latencies, checker output, log highlights) and `summary.json` of one full
`run all --jobs 2` against `celld-uf-7c7d91f` (celld 0.6.3 + fork) with MinIO
`RELEASE.2026-08-04T00-00-00Z`: 18/18 PASS, no `lost_ack`, `duplicate_ack_seq`,
`stale_read` or ordering violation. `chaos_soak` was additionally run with seeds
2, 3 and 4 (all PASS). Raw client logs and node logs are not committed (see the
git-ignored `results/` layout above).

Things the runs showed that are *not* invariant violations:

* Killing the owner costs ~8-12 s of unavailability for its cells (lease TTL
  plus takeover); ~19 s when the bucket is also slow. Requests already in flight
  to the dead owner fail with `DurableObjectRoutingError` (HTTP 500) or hang
  until the client timeout; celld does not replay them.
* SIGTERM/`/shutdown` on one owner hands off in ~1.3 s with exit 0. SIGTERM of
  *all* nodes at once takes the whole `CELLD_SHUTDOWN_TOTAL_MS` (40 s) and logs
  `ERROR ... shutdown_handoff_stalled ... adopting=2` on at least one node
  (cells were handed to a peer that was itself shutting down). No data was lost
  (including after wiping every disk), but a rolling restart is cheaper than a
  simultaneous one.
* A zombie owner resumed after takeover (SIGCONT) self-fenced immediately (exit 3,
  `node_lease_watchdog_fence`); in-flight requests pinned to it were reset and
  none were acknowledged.
* Under sustained load on a busy dev host, nodes often logged `gray follower
  evicted` and ran with a single follower for a few seconds (ensemble
  `members={"n2"}` instead of both peers). Scenarios whose claim needs three
  copies (`kill_two_nodes_wiped`) therefore check the ensemble membership as an
  explicit precondition.
