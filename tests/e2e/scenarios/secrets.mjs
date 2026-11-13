import fs from "node:fs";
import path from "node:path";
import { get, restart, crash, restore, run } from "../lib/dsl.mjs";

// Per-class secrets: the class that declares a secret sees it, nothing else
// does, and it is in neither the deployment nor the local store in the clear.
// `celld dev` reads the value from the example's `.env`, seals it with a key it
// keeps in `.celld/dev`, and the node opens it the way a fleet node does.
const TOKEN = "dev-token-0123456789";
const vault = { token: TOKEN, public: "visible-to-all", hasToken: true, moduleEnvToken: null };
const other = { token: null, public: "visible-to-all", hasToken: false };

const sees = () => [
  get("/vault", { status: 200, json: { ...vault, keys: ["API_TOKEN", "PUBLIC"] } }, { name: "the declaring class sees the secret, among the Worker's vars" }),
  get("/other", { status: 200, json: { ...other, keys: ["PUBLIC"] } }, { name: "another class does not" }),
  get("/", { status: 200, json: { ...other, keys: ["PUBLIC"] } }, { name: "the Worker's own env does not" }),
];

// Every file under a directory, for a plaintext search.
function* files(dir) {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) yield* files(full);
    else if (entry.isFile()) yield full;
  }
}

export default {
  example: "secrets",
  title: "class secrets: one class sees its secret; the value is sealed in the store and absent from the deployment",
  steps: [
    ...sees(),
    run("the secret is in no stored file in the clear", async (t) => {
      const state = path.join(t.workdir, ".celld", "dev");
      let scanned = 0;
      let sealed = false;
      for (const file of files(state)) {
        // The key file holds the key, not the value; every other file (the
        // deployment manifest, the bundle, the sealed secret object) must not
        // hold the value.
        if (path.basename(file) === "secrets.key") continue;
        const bytes = fs.readFileSync(file);
        scanned++;
        t.assert(!bytes.includes(TOKEN), `${file} holds the secret in the clear`);
        if (bytes.includes("aes-256-gcm")) sealed = true;
      }
      t.assert(scanned > 0, "no stored files were found to scan");
      t.assert(sealed, "no sealed secret object was found in the store");
    }),
    restart(),
    ...sees(),
    crash(),
    ...sees(),
    restore(),
    ...sees(),
  ],
};
