import { DurableObject } from "cloudflare:workers";
import addModule from "./add.wasm";

// Each probe reports what the runtime did and never throws, so one request
// shows every way of generating code from a string.
const EMPTY_WASM = new Uint8Array([0, 0x61, 0x73, 0x6d, 1, 0, 0, 0]);

async function probe() {
  const out = {};
  const attempt = async (name, run) => {
    try {
      out[name] = { ok: String(await run()) };
    } catch (error) {
      out[name] = { error: error.name };
    }
  };
  await attempt("eval", () => eval("1 + 1"));
  await attempt("indirectEval", () => (0, eval)("2 + 2"));
  await attempt("Function", () => new Function("a", "return a * 2")(21));
  await attempt("AsyncFunction", () => {
    const AsyncFunction = Object.getPrototypeOf(async () => {}).constructor;
    return new AsyncFunction("return 7")();
  });
  await attempt(
    "WebAssembly.Module",
    () => new WebAssembly.Module(EMPTY_WASM) instanceof WebAssembly.Module,
  );
  await attempt(
    "WebAssembly.compile",
    () =>
      WebAssembly.compile(EMPTY_WASM).then((m) => m instanceof WebAssembly.Module),
  );
  // A module the bundle imports is already compiled: instantiating it is not
  // code generation, and stays available to every class.
  await attempt(
    "importedWasm",
    () => new WebAssembly.Instance(addModule).exports.add(2, 3),
  );
  return out;
}

const LOADED = `
  export default {
    async fetch() {
      try { return Response.json({ ok: String(eval("40 + 2")) }); }
      catch (error) { return Response.json({ error: error.name }); }
    },
  };
`;

async function loaded(env, grant) {
  const name = grant === undefined ? "default" : `codegen-${grant}`;
  const worker = env.LOADER.get(name, () => ({
    compatibilityDate: "2026-01-01",
    mainModule: "main.js",
    modules: { "main.js": LOADED },
    ...(grant === undefined ? {} : { celld: { codegen: grant } }),
  }));
  return (await worker.getEntrypoint().fetch("http://loaded/")).json();
}

class Prober extends DurableObject {
  async fetch(request) {
    const url = new URL(request.url);
    if (url.pathname.endsWith("/loader")) {
      // A request for codegen the loader does not hold is refused at load,
      // not quietly dropped.
      const report = { withoutGrant: await loaded(this.env) };
      try {
        report.requested = await loaded(this.env, true);
      } catch (error) {
        report.requested = { refused: String(error.message ?? error) };
      }
      return Response.json(report);
    }
    return Response.json(await probe());
  }
}

export class Confined extends Prober {}
export class Trusted extends Prober {}

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    const [, who] = url.pathname.split("/");
    if (who === "stateless") return Response.json(await probe());
    const binding = who === "trusted" ? env.TRUSTED : env.CONFINED;
    return binding.getByName("only").fetch(request);
  },
};
