"""Concurrent client workload with a client-side log of every request.

The log is the ground truth the checker works from. For each write the client
records the unique op id, the entry node, start/end time and the outcome:

  ack          HTTP 200 with a body naming our op: the cluster *acknowledged* it
  connect_fail the TCP connect failed: the request was never sent
  timeout      sent, no (complete) response within the client timeout: unknown
  http_<code>  celld/worker answered with an error: not acknowledged
  error        connection reset / garbled after sending: not acknowledged

Only `ack` creates an obligation (the write must survive). Everything else is
"maybe applied" and is allowed either way; the checker reports how many
unacknowledged ops turned out to be present, for information.
"""

from __future__ import annotations

import http.client
import json
import random
import threading
import time

from .util import now


class Workload:
    def __init__(
        self,
        cluster,
        cells: list[str],
        clients: int = 12,
        readers: int = 3,
        pad: int = 0,
        req_timeout: float = 15.0,
        think: float = 0.01,
        seed: int = 1,
        tag: str = "w",
    ):
        self.cluster = cluster
        self.cells = cells
        self.n_clients = clients
        self.n_readers = readers
        self.pad = pad
        self.req_timeout = req_timeout
        self.connect_timeout = 2.0
        self.think = think
        self.seed = seed
        self.tag = tag
        self.writes: list[dict] = []
        self.reads: list[dict] = []
        self.dids: dict[str, str] = {}
        self._stop = threading.Event()
        self._threads: list[threading.Thread] = []
        self._bad_until: dict[str, float] = {}
        self._probes: list["Probe"] = []

    # ------------------------------------------------------------ HTTP
    def _request(self, node_ip: str, method: str, path: str, timeout: float | None = None):
        """Returns (outcome, status, body_json_or_None, error_str)."""
        # Short connect timeout (a load balancer would not wait 15 s on a dead
        # node: a killed container's IP black-holes SYNs); full timeout for the
        # request itself.
        conn = http.client.HTTPConnection(node_ip, 8080, timeout=self.connect_timeout)
        try:
            try:
                conn.connect()
                conn.sock.settimeout(timeout or self.req_timeout)
            except OSError as e:
                return "connect_fail", None, None, repr(e)
            try:
                conn.request(method, path)
                resp = conn.getresponse()
                body = resp.read()
            except TimeoutError as e:
                return "timeout", None, None, repr(e)
            except (OSError, http.client.HTTPException) as e:
                return "error", None, None, repr(e)
            if resp.status != 200:
                return f"http_{resp.status}", resp.status, None, body[:200].decode("utf-8", "replace")
            try:
                return "ok", 200, json.loads(body), ""
            except ValueError as e:
                return "error", 200, None, f"bad json: {e!r}"
        finally:
            conn.close()

    def _pick_node(self, rng: random.Random):
        t = time.monotonic()
        live = [n for n in self.cluster.nodes if self._bad_until.get(n.name, 0) <= t]
        return rng.choice(live or self.cluster.nodes)

    def _penalize(self, node, secs=2.0):
        # Stand-in for a load balancer's health check: avoid a node that just
        # refused/timed out, so a dead node does not eat all client attempts.
        self._bad_until[node.name] = time.monotonic() + secs

    # ------------------------------------------------------------ ops
    def do_write(self, node, cell: str, op: str, client: str, timeout: float | None = None) -> dict:
        t0 = now()
        outcome, status, body, err = self._request(
            node.ip, "POST", f"/append/{cell}?op={op}&pad={self.pad}", timeout
        )
        t1 = now()
        rec = dict(kind="w", op=op, client=client, cell=cell, node=node.name, t0=t0, t1=t1,
                   outcome=outcome, status=status, err=err)
        if outcome == "ok":
            if body.get("op") == op and isinstance(body.get("seq"), int) and body.get("cell") == cell:
                rec.update(outcome="ack", seq=body["seq"], inst=body.get("inst"), did=body.get("did"))
            else:
                rec.update(outcome="error", err=f"unexpected body {body!r}")
        return rec

    def do_read(self, node, cell: str, client: str, timeout: float | None = None) -> dict:
        t0 = now()
        outcome, status, body, err = self._request(node.ip, "GET", f"/head/{cell}", timeout)
        t1 = now()
        rec = dict(kind="r", client=client, cell=cell, node=node.name, t0=t0, t1=t1,
                   outcome=outcome, status=status, err=err)
        if outcome == "ok" and body.get("cell") == cell:
            rec.update(outcome="ok", head=body["head"], count=body["count"], inst=body.get("inst"))
        elif outcome == "ok":
            rec.update(outcome="error", err=f"unexpected body {body!r}")
        return rec

    # ------------------------------------------------------------ warmup
    def warmup(self, per_cell_timeout: float = 60.0):
        """First touch of cell i goes through node i % N so ownership spreads
        over the fleet (a cell is activated on the node that first receives it),
        and learn each cell's Durable Object id for owner discovery."""
        nodes = self.cluster.nodes
        for i, cell in enumerate(self.cells):
            node = nodes[i % len(nodes)]
            deadline = time.monotonic() + per_cell_timeout
            attempt = 0
            while True:
                # a retry needs a fresh op id (the op column is UNIQUE)
                rec = self.do_write(node, cell, f"{self.tag}-warm-{cell}-{attempt}", f"{self.tag}-warm")
                attempt += 1
                self.writes.append(rec)
                if rec["outcome"] == "ack":
                    self.dids[cell] = rec["did"]
                    break
                if time.monotonic() > deadline:
                    raise RuntimeError(f"warmup of cell {cell} via {node.name} failed: {rec}")
                time.sleep(0.5)

    # ------------------------------------------------------------ threads
    def start(self):
        for i in range(self.n_clients):
            t = threading.Thread(target=self._client, args=(i,), daemon=True, name=f"client-{i}")
            self._threads.append(t)
            t.start()
        for i in range(self.n_readers):
            t = threading.Thread(target=self._reader, args=(i,), daemon=True, name=f"reader-{i}")
            self._threads.append(t)
            t.start()

    def _client(self, i: int):
        rng = random.Random(self.seed * 1000 + i)
        name = f"{self.tag}-c{i}"
        n = 0
        while not self._stop.is_set():
            cell = rng.choice(self.cells)
            node = self._pick_node(rng)
            rec = self.do_write(node, cell, f"{name}-{n}", name)
            n += 1
            self.writes.append(rec)
            if rec["outcome"] in ("connect_fail", "timeout", "error"):
                self._penalize(node)
            if self.think:
                time.sleep(self.think * (0.5 + rng.random()))

    def _reader(self, i: int):
        rng = random.Random(self.seed * 7919 + i)
        name = f"{self.tag}-r{i}"
        while not self._stop.is_set():
            cell = rng.choice(self.cells)
            node = self._pick_node(rng)
            rec = self.do_read(node, cell, name)
            self.reads.append(rec)
            if rec["outcome"] in ("connect_fail", "timeout", "error"):
                self._penalize(node)
            time.sleep(0.05 + 0.1 * rng.random())

    def probe(self, node, cells: list[str], label: str, timeout: float = 15.0) -> "Probe":
        p = Probe(self, node, cells, label, timeout)
        self._probes.append(p)
        p.start()
        return p

    def stop(self):
        """Stop issuing requests and wait for in-flight ones to resolve. Bounded
        by the client timeout, after which a hung request has been logged as a
        timeout by its own thread."""
        self._stop.set()
        for p in self._probes:
            p.stop_flag.set()
        for t in self._threads:
            t.join(self.req_timeout + 5)
        for p in self._probes:
            if p.thread:
                p.thread.join(p.timeout + 5)

    # ------------------------------------------------------------ stats
    def acked(self, cell=None):
        return [w for w in self.writes if w["outcome"] == "ack" and (cell is None or w["cell"] == cell)]


class Probe:
    """Pinned-node writer: hammers one node's public listener directly (no
    client-side node choice). Used to ask a suspected zombie to acknowledge."""

    def __init__(self, wl: Workload, node, cells, label, timeout: float = 15.0):
        self.wl, self.node, self.cells, self.label, self.timeout = wl, node, cells, label, timeout
        self.stop_flag = threading.Event()
        self.thread = None

    def start(self):
        self.thread = threading.Thread(target=self._run, daemon=True, name=f"probe-{self.label}")
        self.thread.start()

    def _run(self):
        n = 0
        name = f"{self.wl.tag}-probe-{self.label}"
        while not self.stop_flag.is_set() and not self.wl._stop.is_set():
            for cell in self.cells:
                if self.stop_flag.is_set():
                    break
                rec = self.wl.do_write(self.node, cell, f"{name}-{n}", name, self.timeout)
                self.wl.writes.append(rec)
                n += 1
                if rec["outcome"] == "connect_fail":
                    time.sleep(0.25)  # node is down: do not spin
            time.sleep(0.02)

    def stop(self):
        self.stop_flag.set()
        if self.thread:
            self.thread.join(self.timeout + 5)
