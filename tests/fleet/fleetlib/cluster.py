"""A containerised celld fleet: MinIO + N celld nodes on one docker network.

Why containers: every node gets its own network namespace, so a partition is an
iptables rule inside that namespace, slow links are `tc netem`, and a process
fault is a signal to PID 1 (celld runs as PID 1 on purpose, no init shim, so
SIGSTOP/SIGKILL/SIGTERM hit celld itself).

Resource hygiene: every container and network carries the labels
`celld-fleet=1` and `celld-fleet-run=<id>`; `Cluster.down()` removes by label
and is called from a `finally`. `fleet_clean()` removes leftovers of crashed
runs.
"""

from __future__ import annotations

import json
import os
import random
import shutil
import subprocess
import urllib.request
from dataclasses import dataclass, field
from pathlib import Path

from .util import WaitTimeout, now, run, wait_until

LABEL = "celld-fleet"
MINIO_IMAGE = os.environ.get("MINIO_IMAGE", "pgsty/minio:latest")
NODE_IMAGE = os.environ.get("FLEET_NODE_IMAGE", "celld-fleet-node:1")
ACCESS_KEY = "fleetak"
SECRET_KEY = "fleetsecret123"
BUCKET = "fleet"
HEALTH_PATH = "/.well-known/celld/health"

HERE = Path(__file__).resolve().parent.parent


def docker(*args, check=True, timeout=120):
    return run(["docker", *map(str, args)], check=check, timeout=timeout)


def fleet_clean(run_id: str | None = None, prefix: str | None = None) -> list[str]:
    """Remove harness containers and networks: everything the harness ever
    created (no args), one cluster (`run_id`), or a whole batch (`prefix` of the
    cluster ids, which are `<batch>-<scenario>`)."""
    if prefix is not None:
        flt = f"name=cfleet-{prefix}"
    elif run_id is None:
        flt = f"label={LABEL}=1"
    else:
        flt = f"label={LABEL}-run={run_id}"
    removed = []
    ids = docker("ps", "-aq", "--filter", flt, check=False).stdout.split()
    if ids:
        docker("rm", "-f", "-v", *ids, check=False, timeout=180)
        removed += ids
    nets = docker("network", "ls", "-q", "--filter", flt, check=False).stdout.split()
    for n in nets:
        docker("network", "rm", n, check=False)
        removed.append("net:" + n)
    return removed


def ensure_esbuild() -> str:
    """Return a directory containing `esbuild` (celld deploy needs it on PATH)."""
    env = os.environ.get("FLEET_ESBUILD")
    if env:
        return str(Path(env).parent)
    found = shutil.which("esbuild")
    if found:
        return str(Path(found).parent)
    cache = HERE / ".cache" / "esbuild"
    binp = cache / "node_modules" / ".bin" / "esbuild"
    if not binp.exists():
        cache.mkdir(parents=True, exist_ok=True)
        (cache / "package.json").write_text('{"name":"fleet-esbuild","private":true}')
        run(
            ["npm", "install", "--silent", "--no-audit", "--no-fund", "esbuild@0.25.10"],
            cwd=cache,
            timeout=300,
        )
    return str(binp.parent)


def ensure_node_image():
    if docker("image", "inspect", NODE_IMAGE, check=False).returncode != 0:
        docker("build", "-q", "-t", NODE_IMAGE, HERE / "image", timeout=600)


def check_disk_backed(path: Path):
    t = run(["stat", "-f", "-c", "%T", str(path)]).stdout.strip()
    if t in ("tmpfs", "ramfs") and not os.environ.get("FLEET_ALLOW_TMPFS"):
        raise RuntimeError(
            f"scratch dir {path} is on {t}; set FLEET_WORK to a disk-backed path "
            "(celld's durability claims are about fsync'd disks)"
        )
    return t


def http(url, method="GET", timeout=5.0, data=None):
    req = urllib.request.Request(url, method=method, data=data)
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return r.status, r.read()


@dataclass
class Node:
    cluster: "Cluster"
    name: str  # n1, n2, ...
    ip: str
    cname: str
    datadir: Path
    starts: int = 0
    killed_at: float | None = None

    # ---- addressing
    @property
    def public(self) -> str:
        return f"http://{self.ip}:8080"

    @property
    def internal(self) -> str:
        return f"http://{self.ip}:8081"

    # ---- lifecycle
    def create(self):
        c = self.cluster
        self.datadir.mkdir(parents=True, exist_ok=True)
        env = {
            "AWS_ACCESS_KEY_ID": ACCESS_KEY,
            "AWS_SECRET_ACCESS_KEY": SECRET_KEY,
            "AWS_REGION": "us-east-1",
            "CELLD_NODE": self.name,
            "CELLD_WATCH": "/var/lib/celld/state",
            "RUST_LOG": c.rust_log,
            "CELLD_TTL_MS": str(c.ttl_ms),
            **c.extra_env,
        }
        args = [
            "run", "-d", "--name", self.cname,
            "--label", f"{LABEL}=1", "--label", f"{LABEL}-run={c.run_id}",
            "--network", c.net, "--ip", self.ip,
            "--user", f"{os.getuid()}:{os.getgid()}",
            "--cap-add", "NET_ADMIN",
            "--log-opt", "max-size=100m",
            "-v", f"{c.bin}:/celld:ro",
            "-v", f"{self.datadir}:/var/lib/celld",
        ]
        for k, v in env.items():
            args += ["-e", f"{k}={v}"]
        args += [
            NODE_IMAGE, "/celld",
            "--bucket", f"s3://{BUCKET}",
            "--endpoint", f"http://{c.minio_ip}:9000",
            "--listen", "0.0.0.0:8080",
            "--internal-listen", f"{self.ip}:8081",
            "--advertise", f"{self.ip}:8081",
        ]
        docker(*args)
        self.starts += 1

    def start(self, wipe: bool = False):
        """(Re)start the container with the same CELLD_NODE identity. `wipe`
        deletes the node's local disk first: a replacement machine."""
        if wipe:
            shutil.rmtree(self.datadir, ignore_errors=True)
            self.datadir.mkdir(parents=True, exist_ok=True)
        docker("start", self.cname)
        self.starts += 1

    def inspect(self) -> dict:
        r = docker("inspect", self.cname, "--format", "{{json .State}}", check=False)
        return json.loads(r.stdout) if r.returncode == 0 and r.stdout.strip() else {}

    def status(self) -> str:
        return self.inspect().get("Status", "missing")

    def exit_code(self):
        st = self.inspect()
        return st.get("ExitCode") if st.get("Status") == "exited" else None

    def signal(self, sig: str):
        docker("kill", "-s", sig, self.cname)

    def kill9(self):
        self.killed_at = now()
        self.signal("KILL")

    def pause(self):
        self.signal("STOP")

    def resume(self):
        self.signal("CONT")

    def proc_state(self) -> str:
        """`ps` state of PID 1 (T = stopped)."""
        r = docker("exec", self.cname, "ps", "-o", "stat=", "-p", "1", check=False)
        return r.stdout.strip()

    def wait_exited(self, timeout: float) -> int:
        wait_until(lambda: self.status() == "exited", timeout, f"{self.name} to exit")
        return self.exit_code()

    def healthy(self) -> bool:
        try:
            s, _ = http(self.public + HEALTH_PATH, timeout=2)
            return s == 200
        except Exception:
            return False

    def wait_healthy(self, timeout: float):
        wait_until(self.healthy, timeout, f"{self.name} health 200")

    def state(self) -> dict:
        s, b = http(self.internal + "/state", timeout=3)
        return json.loads(b)

    def residents(self) -> list[str]:
        try:
            return self.state().get("residents", [])
        except Exception:
            return []

    def shutdown_api(self, preserve=False):
        q = "?handoff=preserve" if preserve else ""
        return http(self.internal + "/shutdown" + q, method="POST", timeout=5, data=b"")

    # ---- network faults (executed as root inside the node's netns)
    def _sh(self, script: str):
        return docker("exec", "--user", "0", self.cname, "sh", "-ec", script, timeout=30)

    def partition_from(self, ips: list[str]):
        """Drop all packets to/from `ips` (silent drop, a true partition)."""
        for ip in ips:
            self._sh(f"iptables -I INPUT -s {ip} -j DROP; iptables -I OUTPUT -d {ip} -j DROP")

    def heal(self):
        """Remove partition rules. A node that already exited (self-fence) has
        no network namespace left, so there is nothing to heal."""
        if self.status() == "running":
            self._sh("iptables -F INPUT; iptables -F OUTPUT")

    def netem(self, ips: list[str], delay_ms: int, jitter_ms: int = 0):
        """Delay egress packets destined for `ips` (other traffic untouched)."""
        self.clear_netem()
        cmds = [
            "tc qdisc add dev eth0 root handle 1: prio bands 3 priomap " + " ".join(["0"] * 16),
            f"tc qdisc add dev eth0 parent 1:3 handle 30: netem delay {delay_ms}ms {jitter_ms}ms",
        ]
        for ip in ips:
            cmds.append(
                f"tc filter add dev eth0 protocol ip parent 1: prio 1 u32 match ip dst {ip}/32 flowid 1:3"
            )
        self._sh("; ".join(cmds))

    def clear_netem(self):
        docker("exec", "--user", "0", self.cname, "tc", "qdisc", "del", "dev", "eth0", "root", check=False)

    def ensemble_members(self):
        """Followers of this node's current log ensemble, from its latest
        `log ensemble open ... members={...}` line, or None if the ensemble is
        degraded (acks ride the bucket) or never opened."""
        import re
        cur = None
        for ln in self.logs().splitlines():
            m = re.search(r'log ensemble open; fleet acks enabled epoch=\d+ members=\{(.*?)\}', ln)
            if m:
                cur = sorted(re.findall(r'"([^"]+)"', m.group(1)))
            elif "log ensemble degraded" in ln:
                cur = None
        return cur

    def logs_contain(self, needle: str) -> bool:
        return needle in self.logs()

    def logs(self, since: str | None = None) -> str:
        a = ["logs", "-t"]
        if since:
            a += ["--since", since]
        r = docker(*a, self.cname, check=False, timeout=60)
        return r.stdout + r.stderr


class Cluster:
    def __init__(
        self,
        run_id: str,
        workdir: Path,
        bin_path: str,
        nodes: int = 3,
        ttl_ms: int = 10000,
        rust_log: str = "warn,celld::node_log=info",
        extra_env: dict | None = None,
    ):
        self.run_id = run_id
        self.workdir = workdir
        self.bin = str(Path(bin_path).resolve())
        self.n_nodes = nodes
        self.ttl_ms = ttl_ms
        self.rust_log = rust_log
        self.extra_env = extra_env or {}
        self.net = f"cfleet-{run_id}"
        self.subnet = None
        self.minio_ip = None
        self.minio_name = f"cfleet-{run_id}-minio"
        self.nodes: list[Node] = []
        self.logdir = workdir / "logs"
        self._up = False

    # ---- context manager = the finally path
    def __enter__(self):
        return self

    def __exit__(self, *exc):
        self.down()
        return False

    # ---- bring-up
    def up(self):
        self.workdir.mkdir(parents=True, exist_ok=True)
        check_disk_backed(self.workdir)
        ensure_node_image()
        self.logdir.mkdir(exist_ok=True)
        self._up = True  # from here on down() has things to remove
        self._create_network()
        self._start_minio()
        self._deploy()
        for i in range(self.n_nodes):
            n = Node(
                cluster=self,
                name=f"n{i + 1}",
                ip=f"{self.subnet}.{11 + i}",
                cname=f"cfleet-{self.run_id}-n{i + 1}",
                datadir=self.workdir / f"n{i + 1}",
            )
            self.nodes.append(n)
            n.create()
        # Bound: CELLD_READY_FLEET_GATE_MS defaults to 120 s; a healthy 3-node
        # start opens the gate within ~2 s, so 90 s only trips on a real fault.
        for n in self.nodes:
            n.wait_healthy(90)

    def _create_network(self):
        rng = random.Random(self.run_id)
        last = None
        for _ in range(40):
            a, b = rng.randint(150, 199), rng.randint(0, 255)
            sub = f"10.{a}.{b}"
            r = docker(
                "network", "create", "--subnet", f"{sub}.0/24",
                "--label", f"{LABEL}=1", "--label", f"{LABEL}-run={self.run_id}",
                self.net, check=False,
            )
            if r.returncode == 0:
                self.subnet = sub
                self.minio_ip = f"{sub}.2"
                return
            last = r.stderr
        raise RuntimeError(f"could not allocate a docker subnet: {last}")

    def _start_minio(self):
        d = self.workdir / "minio"
        (d / BUCKET).mkdir(parents=True, exist_ok=True)  # a directory is a bucket
        docker(
            "run", "-d", "--name", self.minio_name,
            "--label", f"{LABEL}=1", "--label", f"{LABEL}-run={self.run_id}",
            "--network", self.net, "--ip", self.minio_ip,
            "--user", f"{os.getuid()}:{os.getgid()}",
            "-e", f"MINIO_ROOT_USER={ACCESS_KEY}", "-e", f"MINIO_ROOT_PASSWORD={SECRET_KEY}",
            "-v", f"{d}:/data",
            MINIO_IMAGE, "server", "/data", "--address", ":9000", "--console-address", ":9001",
        )

        def ready():
            s, _ = http(f"http://{self.minio_ip}:9000/minio/health/ready", timeout=2)
            return s == 200

        wait_until(ready, 60, "MinIO ready")

    def deploy_env(self):
        env = dict(os.environ)
        env.update(
            AWS_ACCESS_KEY_ID=ACCESS_KEY,
            AWS_SECRET_ACCESS_KEY=SECRET_KEY,
            AWS_REGION="us-east-1",
        )
        env["PATH"] = ensure_esbuild() + os.pathsep + env.get("PATH", "")
        return env

    def _deploy(self):
        r = run(
            [self.bin, "deploy", str(HERE / "app"), "--bucket", f"s3://{BUCKET}",
             "--endpoint", f"http://{self.minio_ip}:9000"],
            env=self.deploy_env(), timeout=120,
        )
        self.deploy_output = r.stdout.strip().splitlines()[-1] if r.stdout.strip() else ""

    def diagnose(self) -> str:
        r = run(
            [self.bin, "diagnose", "--bucket", f"s3://{BUCKET}",
             "--endpoint", f"http://{self.minio_ip}:9000"],
            env=self.deploy_env(), timeout=120, check=False,
        )
        return r.stdout + r.stderr

    # ---- queries
    def node(self, name: str) -> Node:
        return next(n for n in self.nodes if n.name == name)

    def others(self, n: Node) -> list[Node]:
        return [x for x in self.nodes if x is not n]

    def owner_map(self, dids: dict[str, str]) -> dict[str, str | None]:
        """cell -> node name currently holding the cell resident (None if none)."""
        res = {n.name: set(n.residents()) for n in self.nodes if n.status() == "running"}
        out = {}
        for cell, did in dids.items():
            key = f"Ledger:{did}"
            out[cell] = next((nn for nn, r in res.items() if key in r), None)
        return out

    def ips_of(self, nodes) -> list[str]:
        return [n.ip for n in nodes]

    # ---- teardown
    def down(self):
        if not self._up:
            return
        self._up = False
        # Heal first is unnecessary (containers are removed), but logs are
        # valuable evidence: dump before removal.
        for n in self.nodes:
            try:
                (self.logdir / f"{n.name}.log").write_text(n.logs())
            except Exception:
                pass
        try:
            r = docker("logs", "-t", self.minio_name, check=False)
            (self.logdir / "minio.log").write_text(r.stdout + r.stderr)
        except Exception:
            pass
        fleet_clean(self.run_id)
        # Node and MinIO data. Containers ran as our uid, so plain rmtree works.
        if not os.environ.get("FLEET_KEEP"):
            for sub in ["minio"] + [n.name for n in self.nodes]:
                shutil.rmtree(self.workdir / sub, ignore_errors=True)
