import { DurableObject } from "cloudflare:workers";

// A `blockConcurrencyWhile` callback that runs for 35 s. Cloudflare resets the
// object after 30 s, and celld does the same by default. A node started with
// --no-implicit-deadlines lets the callback finish.
export class Slow extends DurableObject {
  async fetch() {
    const started = Date.now();
    try {
      await this.ctx.blockConcurrencyWhile(
        () => new Promise((resolve) => setTimeout(resolve, 35_000)),
      );
      return Response.json({ outcome: "completed", seconds: Math.round((Date.now() - started) / 1000) });
    } catch (error) {
      return Response.json({ outcome: "reset", message: String(error.message ?? error) });
    }
  }
}

export default {
  fetch(request, env) {
    return env.SLOW.getByName("only").fetch(request);
  },
};
