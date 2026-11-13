"""Small shared helpers: clock, bounded waiting, shell-out."""

from __future__ import annotations

import subprocess
import time

_T0 = time.monotonic()
_WALL0 = time.time()


def now() -> float:
    """Seconds since harness import, monotonic. All client-log timestamps use
    this one clock (single process), so real-time ordering checks never depend
    on wall-clock steps or on clocks inside the containers."""
    return time.monotonic() - _T0


def wall(t: float) -> str:
    """Render a `now()` value as a UTC wall-clock string for humans."""
    return time.strftime("%H:%M:%S", time.gmtime(_WALL0 + t)) + f".{int((t % 1) * 1000):03d}"


class WaitTimeout(Exception):
    pass


def wait_until(pred, timeout: float, what: str, poll: float = 0.25):
    """Poll `pred()` until it returns a truthy value (which is returned).

    This is the only synchronisation primitive scenarios use for correctness:
    every wait names the condition it waits for and has an upper bound that is
    chosen from a documented celld constant (lease TTL, recovery retries...),
    never "sleep long enough". Running out of time raises WaitTimeout, which a
    scenario reports as a failed liveness expectation rather than hanging.
    """
    deadline = time.monotonic() + timeout
    last_err = None
    while True:
        try:
            v = pred()
            if v:
                return v
            last_err = None
        except Exception as e:  # predicate may probe a dead node
            last_err = e
        if time.monotonic() >= deadline:
            raise WaitTimeout(
                f"timed out after {timeout:.0f}s waiting for: {what}"
                + (f" (last error: {last_err!r})" if last_err else "")
            )
        time.sleep(poll)


def run(args, check=True, timeout=120, input=None, env=None, cwd=None):
    r = subprocess.run(
        args,
        capture_output=True,
        text=True,
        timeout=timeout,
        input=input,
        env=env,
        cwd=cwd,
    )
    if check and r.returncode != 0:
        raise RuntimeError(
            f"command failed ({r.returncode}): {' '.join(map(str, args))}\n"
            f"stdout: {r.stdout[-2000:]}\nstderr: {r.stderr[-2000:]}"
        )
    return r
