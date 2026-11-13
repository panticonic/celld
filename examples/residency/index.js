// A Durable Object that keeps live, in-memory state across idle periods.
//
// `heap` is plain JavaScript memory: nothing about it is stored. Without a
// hold, celld evicts an idle object (CELLD_IDLE_EVICT_S, or memory pressure),
// the isolate's memory goes with it, and the next request starts from zero.
// With a hold, the object stays resident until it calls `release()`.
export class Kernel {
  constructor(ctx, env) {
    this.ctx = ctx;
    this.heap = { counter: 0, bornAt: Date.now() };
    this.hold = null;
  }

  async fetch(request) {
    const { pathname } = new URL(request.url);
    switch (pathname) {
      case "/inc":
        this.heap.counter++;
        break;
      case "/hold":
        await this.acquire();
        break;
      case "/release":
        this.hold?.release();
        this.hold = null;
        break;
    }
    return Response.json({
      counter: this.heap.counter,
      // Changes when the object was evicted and constructed again.
      bornAt: this.heap.bornAt,
      held: this.hold !== null,
      // Set by `onrevoked` when the node took the hold away (a drain, a swap or
      // an operator): the reason, and the counter it saved.
      revoked: (await this.ctx.storage.get("lastRevocation")) ?? null,
      note: this.note,
    });
  }

  async acquire() {
    if (this.hold !== null) return;
    try {
      this.hold = await this.ctx.residency.hold();
    } catch (error) {
      // The node refused: the class did not declare residency, or the node's
      // pin budget is spent. Nothing was retained.
      this.note = `${error.code}: ${error.message}`;
      return;
    }
    // Runs when the node is about to take the cell away (a drain, a generation
    // swap, an operator). Persist whatever must survive; the node waits for
    // this handler for at most CELLD_HOLD_GRACE_MS.
    this.hold.onrevoked = async ({ reason }) => {
      await this.ctx.storage.put("lastRevocation", { reason, counter: this.heap.counter });
    };
    this.hold.released.then(() => {
      this.hold = null;
    });
  }
}

export default {
  async fetch(request, env) {
    const name = new URL(request.url).searchParams.get("name") ?? "default";
    return env.KERNEL.get(env.KERNEL.idFromName(name)).fetch(request);
  },
};
