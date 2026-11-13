// Per-class secrets: API_TOKEN is declared for the class `Vault` in
// wrangler.jsonc, so only a `Vault` object's `env` has it. The Worker's own
// `env`, the class `Other`, and the `env` of `cloudflare:workers` do not.
import { env as moduleEnv } from "cloudflare:workers";

const report = (env, extra = {}) => ({
  token: env.API_TOKEN ?? null,
  public: env.PUBLIC ?? null,
  hasToken: "API_TOKEN" in env,
  keys: Object.keys(env).filter((key) => key !== "VAULT" && key !== "OTHER").sort(),
  ...extra,
});

export class Vault {
  constructor(ctx, env) {
    this.env = env;
  }
  async fetch() {
    // The module-level env is the Worker's: no secret in it.
    return Response.json(report(this.env, { moduleEnvToken: moduleEnv.API_TOKEN ?? null }));
  }
}

export class Other {
  constructor(ctx, env) {
    this.env = env;
  }
  async fetch() {
    return Response.json(report(this.env));
  }
}

export default {
  async fetch(request, env) {
    const path = new URL(request.url).pathname;
    if (path === "/vault") return env.VAULT.getByName("v").fetch(request);
    if (path === "/other") return env.OTHER.getByName("o").fetch(request);
    return Response.json(report(env));
  },
};
