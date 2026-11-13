# Per-class secrets

A Durable Object class can have secrets that only it sees. They are not Worker
variables: they are not in the deployment manifest, not in the bundle, and not
visible to the Worker's `fetch`, to another class, or to `cloudflare:workers`.
This is the fork's answer to "a secret for one class, not stored in plain
deployment variables in the bucket index" (backlog G3, Vibestudio).

The example is [`examples/secrets`](../../examples/secrets), and its scenario in
`tests/e2e/scenarios/secrets.mjs` runs it.

## Declare

In the Wrangler config, the `celld` block lists the names a class sees:

```jsonc
"celld": {
  "durable_objects": {
    "Vault": { "secrets": ["API_TOKEN", "DB_URL"] }
  }
}
```

- A name is a JavaScript binding name (`API_TOKEN`).
- A secret belongs to one class: the same name under two classes is refused, as
  is a name that is also a `vars` entry or a binding.
- The class must be one of the Worker's Durable Object classes.
- Only the names are in the manifest. A deployment that declares secrets
  requires the `class-secrets-v1` feature, so a node that predates it refuses the
  deployment instead of running the class without its credential.

## Store the values

```sh
celld secret keygen > secrets.key                 # once; keep it out of the bucket
export CELLD_SECRETS_KEY_FILE=secrets.key
celld secret put my-worker API_TOKEN --value "$TOKEN" --bucket s3://fleet
celld secret put my-worker DB_URL --file db-url.txt --bucket s3://fleet
printf %s "$VALUE" | celld secret put my-worker NAME --pipe --bucket s3://fleet
celld secret list my-worker --bucket s3://fleet    # names only
celld secret delete my-worker NAME --bucket s3://fleet
```

`my-worker` is the config's `name`. The value is UTF-8 text of at most 64 KiB.
`--bucket`, `--endpoint` and `--region` are the same fleet flags as the other
operator commands.

Each value is stored as its own object, `secrets/<script>/<NAME>`, outside every
deployment, as an AES-256-GCM record:

```json
{ "v": 1, "alg": "aes-256-gcm", "nonce": "…", "ct": "…" }
```

The key is not in the bucket. The script name and the secret name are the
associated data, so a stored value cannot be copied to another name or script and
still open. A wrong key, another name and an edited object fail the same way,
with one message. Reading the bucket (an index, a listing, a backup) reveals no
secret.

## Give nodes the key

Every node, and `celld secret put`, needs the same 32-byte key, from one of:

| Setting | Meaning |
| --- | --- |
| `--secrets-key-file PATH` / `CELLD_SECRETS_KEY_FILE` | A file with the key: 32 raw bytes, or the text `keygen` prints (base64, or 64 hex digits). |
| `CELLD_SECRETS_KEY` | The same text, in the environment. |

A node opens the values when it **loads a deployment**: at start, and when it
adopts a new version. If a deployment declares secrets and the node has no key,
or a declared secret is not stored, or a value does not open, the node does not
load the deployment and says which. It never serves the class with a credential
missing, because the application would see `undefined` where it expects one.
`celld deploy` checks that every declared secret is stored (it needs no key, only
the bucket) and refuses to publish otherwise:

```
Error: the deployment declares secrets the bucket does not hold: API_TOKEN
(class Vault). Set each with `celld secret put my-worker NAME`
```

**Rotating a value** is `celld secret put` again followed by a new load: the
next `celld deploy`, or a node restart. A running node keeps the value it
opened. Rotating the key means putting every value again under the new key and
restarting the nodes with it.

## What the class sees

The constructor of a declaring class gets an `env` that is the Worker's `env`
plus its secrets:

```js
export class Vault extends DurableObject {
  constructor(ctx, env) {
    super(ctx, env);
    this.token = env.API_TOKEN;      // set
    env.PUBLIC;                      // the Worker's vars and bindings are there too
  }
}
```

The same object is what `this.env` holds, so `Object.keys(env)`, `"API_TOKEN" in
env` and `env.API_TOKEN` all see it, and it cannot be overwritten or deleted. No
other `env` has it: the Worker's `fetch(request, env)`, another class's
constructor, an entrypoint class, or `import { env } from "cloudflare:workers"`.
A facet gets the secrets of its own class and not its parent's, and a Dynamic
Worker gets none; pass a value on purpose if one needs it. The values reach the isolate as V8 values and
are in no source text, so they are not in a compiled script, the inspector's
script list, or a log line (`ClassSecrets` prints names only).

## `celld dev`

Wrangler's local secrets are `.dev.vars`; so are these. A name declared as a
class secret is read from `.dev.vars` (or `.env`, `.env.local`) like a variable
and **removed from the variables**: it never enters the deployment. `celld dev`
seals it into its local store with a key it creates in `.celld/dev/secrets.key`
(mode 0600) and hands the node that file, so the node opens it the way a fleet
node does. A declared secret with no value in those files stops the build, naming
the file to edit. `celld deploy` reads none of these files, so a development
value cannot reach a fleet.

## Not covered

- Worker-level secrets, shared by every class: use `vars`, or declare the secret
  for the one class that needs it.
- Reading a value back: there is no `celld secret get`. A holder of the key can
  open a stored object; the CLI does not offer it.
- Hardware or KMS-held keys. The key is a file or an environment variable.
- Secrets for Dynamic Workers, Workflows, and entrypoint (non-Durable-Object)
  classes.
