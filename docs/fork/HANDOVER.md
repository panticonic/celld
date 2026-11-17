# Handover: the unified celld fork, Vibestudio and Chronolog (2026-11-15)

This is the entry point for an agent that takes over the whole project. It
says what the project is and what the owner decided, where every piece of
work lives, what is done and how it was verified, and what is left, in order.
The detailed per-stream handoffs it points to are authoritative for their own
details; this file is authoritative for the overall order and the rules.

**Continuation, 2026-11-17.** P4 verification resumed in the feature worktrees.
Host `feat/dolt-p0` is now `9549eeccb`; both system-testing feature branches are
at `ea84ad3`. Read "P4 continuation (2026-11-17)" in
`~/vibestudio-worktrees/dolt-p0/HANDOFF-dolt-redesign.md` before the older pass
records. Four defects were fixed and committed locally:

- Host `f2fa90aa2`: a scoped native extension can explicitly materialize its
  absent context projection. The old readiness guard rejected the operation
  intended to establish readiness. The workspace test now passes on the real
  stack, as does file search/read; all 77 filesystem tests pass.
- System-testing `f86aef1`: permission-read validators use native inventory
  receipts instead of requiring an array-shaped guest result. The archived
  final-sweep authority failure was a false negative for `{count, rows}`.
- System-testing `d21afc8`: parse each eval in its own lexical scope; repeated
  local declarations across separate cells are valid. The DO probe now passes.
- System-testing `ea84ad3`: join installed-consumer calls by native target
  receipts; a handle retained in `scope` need not use its creation cell's local
  variable. The actual captured execution passes revalidation.

The stopped sweep's five failures have each had their required rerun: four pass;
`worker-env` still has the known model descriptor-export error (the model then
recovered, observed the value and cleaned up). The two unfinished worker cases
are verified after the validator repairs. Harness resilience is **5/5 green**.
The focused validator suites pass (91 and 25 tests); the three captured failing
executions all pass revalidation; host and template typechecks and lint pass.
The full commit gate fails only the documented formatting baseline in eight
untouched host files. No input-gate or saturation warnings in the six instances
used so far. The fork runtime and binary pin remain `75f1700`.

**Next:** finish panels and subagents (including `steer-a-running-child`), rerun
new failures once and inspect them, then continue §5.1 items 2–5. The saturation
and S6b investigations, compaction soak, WorkspaceStateDO compaction, restart
probe and 10k-turn fork measurement remain open. Nothing outside the fork has
been pushed; the owner's main checkouts remain untouched.

Read in this order: §1 (goal and decisions), §2 (rules), §3 (map of repos),
§4 (state per stream), §5 (the plan to finish), §6 (how to run things),
§7 (lessons).

---

## 1. Goal and the owner's decisions

**Goal.** One runtime, a fork of celld, that exceeds what both
`~/vibestudio` and `~/chronolog-db` need. It replaces Vibestudio's bundled
workerd and hosts a Chronolog node, so that Chronolog can be embedded in
Vibestudio. Then a general refactor of Vibestudio onto Dolt-backed cells.

- **Fork repository.** github.com/panticonic/celld, branch `unified-fork`,
  local remote name `panticonic`. It is a public fork of denoland/celld.
- **Design.** The fork's design is `docs/fork/design.md`. The Vibestudio
  redesign is `docs/dolt-redesign.md` in the Vibestudio worktree (§3).

Decisions (dates are when the owner made them; none of these are open):

- **2026-11-13, runtime and Chronolog**
  - DoltLite 0.51.11 is the only SQLite in the binary. Stock format is the
    default; prolly-mode files are Dolt cells.
  - Chronolog has one host, the fork. `chronologd` and the N-API addon are
    retired.
  - One fork node per Vibestudio workspace.
  - Chronolog's peer transport is Iroh built into the fork, replacing SSB
    (secret-stack, SHS, muxrpc, ssb-keys, ssb-caps).
- **2026-11-13, the Vibestudio redesign**
  - GAD and the agent system are refactored onto Dolt cells.
  - **GAD is decentralized and deeply Dolt-integrated.** One Dolt database
    if size allows. Participants (agents, the user, contexts, later devices
    and other workspaces) work in parallel on their own forks or branches and
    merge back by fetch, push and merge. No single object serializes the
    work.
  - Q1: a Dolt merge adopts non-conflicting changes; decisions are recorded
    only for conflicts.
  - Q2: replay GAD's full history into Dolt at migration.
  - Q3: an eval heap is held until an explicit end; there is no idle timer.
  - Q4: agents sign Chronolog writes with their user's key after approval.
  - Q5: stores that hold secrets (`BrowserVaultDO`, `WebhookStoreDO`) stay
    SQLite.
- **2026-11-14, dynamic code**
  - No eval special case. There is one general system for dynamic code that
    can be hosted in any kind of entity: agent, panel, CLI session, worker.
  - One code-hosting helper (`hostCode`). One admission step
    (`dynamicCode.admit` / `release`).
  - Eval is an ordinary kernel class.
  - Results are pulled by run id; push and redelivery are deleted.
  - D5: legacy EvalDO state is dropped at migration.
- **2026-11-14, templates.** The uncommitted edits found in the
  `~/vibestudio-templates/*` main checkouts had been forgotten. They are now
  committed locally on `main` and not pushed: base `d79b61f`, system-testing
  `f611e7b`, personal `3274225`, system `0568073`.

---

## 2. Rules for working on this project (all still in force)

- **Single-track.** Run one stream at a time and do not fan out. A subagent
  may do one stream; review and merge its result before starting the next.
- **Agent models.** Use Sonnet subagents for straightforward work. Use Opus
  sparingly, for hard concurrency or runtime bugs; it was used three times,
  and each one was a fork concurrency bug.
- **Do not touch:**
  - `~/vibestudio`, the owner's main checkout and running instance;
  - the main checkouts of the templates (`~/vibestudio-templates/*`) and of
    Pi (`~/vibestudio-pi-durable-fork`), except as noted in §1;
  - the owner's processes: the Vibestudio app, llama-server, postgres, a
    vitest in `~/namlu`, gnome-software.
- **Push and publish.**
  - The fork **is** pushed: `git push panticonic unified-fork`, after a
    green e2e.
  - Vibestudio, the templates, Pi and Chronolog are **never** pushed or
    published by agents; the owner does that. Everything there is local
    commits on feature branches.
- **Killing processes.** Kill by PID only. Never use `pkill`, `pkill -f` or
  `killall`: agents have killed their own shells this way, and the owner's
  processes share names. Before starting test lanes, check with `ps` that no
  earlier lanes script or instance of yours is still running; a stray one has
  invalidated a sweep twice.
- **Memory caps.** The box has 30 GB, and it hit a global OOM once with
  about 10 parallel builds. Cap every heavy build or test:
  `systemd-run --user --scope --quiet -p MemoryMax=6G -p MemorySwapMax=0 env CARGO_BUILD_JOBS=3 …`.
  Vibestudio system-test instances need about 150 s between groups to stay
  under the 6 GiB scope.
  For TypeScript 7's native compiler, also set `GOMEMLIMIT=3GiB GOMAXPROCS=3`:
  the template typecheck otherwise exceeded the scope in the continuation.
  Node heap limits do not control that compiler.
- **Waits.** Every wait has a timeout. A subagent must not end its turn while
  its own background work is running; stop it with TaskStop if it lingers.
- **AGENTS.md.** The Vibestudio `AGENTS.md` rules (for example "no parallel
  paths" and "no two transports") are not the owner's own rules. Cite them
  where they apply, but weigh them on the merits.
- **Root causes, not retries.** A retry that hides a refusal is not a fix;
  E14 was found only after a 40-attempt retry was rejected. Fix the fork when
  the fork is wrong: write a black-box e2e repro first, fix, document an
  E-entry, merge, push, and pin the new binary in Vibestudio.

---

## 3. Map: repositories, branches, worktrees, binaries

### 3.1 The fork, `/home/werg/celld`

- **Branch:** `unified-fork`, at `75f1700`, pushed to `panticonic`.
- **Rust:** 1.94.1 at `~/.cargo/bin`; the system rustc 1.93 is too old.
- **Docs:**
  - `docs/fork/design.md`: status table, single-track backlog, and §3b,
    the decisions as built (§3b.1–§3b.18).
  - `docs/fork/upstream-findings.md`: defects S1–S5 and E1–E14.
  - `docs/services/*.md`: per-feature docs, including dolt-cells,
    dolt-remotes, residency, dynamic-workers, p2p, object-fork and
    durable-objects.
  - `docs/fork/class-secrets.md`, `docs/fork/codegen.md`, and others.
- **Tests:**
  - `tests/e2e`: a black box against `examples/*`, 49 scenarios.
  - `tests/fleet`: fault injection.
  - `crates/logic-sim`: the simulator.
  - Upstream ships no tests: its `cfg(celld_internal_tests)` bodies are
    stripped.
- **Binaries:** `~/celld-harness-bin/celld-uf-<sha>`, one per pinned fork
  commit. The latest is `celld-uf-75f1700`.
- **Stale worktrees:**
  - `.claude/worktrees/agent-a5af5c507bdc8c800` is locked by a dead agent and
    can be removed with `git worktree remove -f -f`.
  - `.claude/worktrees/agent-a142d5fa71f18d916` (`fix/retired-input-gate`) is
    merged and can be removed.
  - `~/celld-worktrees/cutover-bin` (detached `fffaac1`) is old.

### 3.2 Vibestudio host

- **Main checkout:** `~/vibestudio`. Do not touch it.
- **`~/vibestudio-worktrees/celld-cutover`**, branch `feat/celld-runtime`,
  head `db8e365fd`. The runtime cutover from workerd to the fork. Handoff:
  `HANDOFF-celld-runtime.md`.
- **`~/vibestudio-worktrees/dolt-p0`**, branch `feat/dolt-p0`. **This is the
  live branch.**
  - It contains `feat/celld-runtime` and is 97 commits ahead of `main`
    (`f9675e14e`).
  - **Handoff:** `HANDOFF-dolt-redesign.md`, the authoritative per-phase
    record. **Design:** `docs/dolt-redesign.md`.
  - **Fork pin:** `native/celld/targets.json`, holding `fork.commit` and a
    sha256 per target. Only linux-x64 is filled. The staged binary is
    `native/celld/linux-x64/celld` (gitignored).
- **Older worktrees:** `~/vibestudio-worktrees/dolt-redesign` (branch
  `design/dolt-redesign`) holds the design only and is superseded by
  `dolt-p0`. Branch `audit/celld-compat` holds `docs/celld-compat-audit.md`.

### 3.3 Vibestudio templates

These are userland: Base, system-testing and the others. The main checkouts
are in `~/vibestudio-templates/*`; the worktrees are in
`~/vibestudio-templates-worktrees/`.

- **`base-dolt-p2`**, branch `feat/dolt-p2`: Base, including the agents
  (`packages/agentic-do`), the channel (`workers/pubsub-channel`), the eval
  kernel (`workers/eval-kernel`) and sessions (`workers/session`).
- **`base-dolt-p4`**, branch `feat/dolt-p2-s3`: the same head as
  `feat/dolt-p2`, used for the P4 lanes.
- **`system-testing-dolt-p2`** (`feat/dolt-p2`) and
  **`system-testing-dolt-p4`** (`feat/dolt-p2-s4`): the system-test scenarios.
- **`examples-`, `google-workspace-`, `news-`, `personal-` and
  `system-dolt-p4`**, branch `feat/dolt-p4-pi`: one commit each, pinning the
  local Pi release.
- **Symlink roots** `root-dolt-p2` and `root-dolt-p4`: a host instance reads
  the templates through `VIBESTUDIO_TEMPLATE_CHECKOUTS=<root>`. The roots
  point at the worktrees above; `spectrolite`, `state` and
  `system-stream-integrity-worktree` point at the main checkouts, unchanged.

### 3.4 Pi (`@panticonic/pi-durable`)

- **Main checkout:** `~/vibestudio-pi-durable-fork`. Do not touch it. Its
  remotes are `origin` (git@github.com:werg/pi-harness.git) and `upstream`
  (earendil-works/pi).
- **Worktree:** `~/vibestudio-pi-worktrees/dolt-p4`, branch `feat/dolt-p4`.
  It adds `CompositeSqliteStorage` (a versioned and a local table set, one id
  counter, one transaction), a commit option on the native transaction, and
  local migrations that re-run.
  - The retry for input-gate refusals was added (`611e057aa`) and then
    removed (`c6d518177`) once the fork fixed E14.
- **How Base consumes it:** a locally packed prerelease (currently
  `1.2.0-vibestudio.4-dolt.5`), built with `package-vibestudio.mjs`.
  - The host pins it with `file:` overrides.
  - Userland builds need `VIBESTUDIO_DEV_NPM_REGISTRY=http://127.0.0.1:4873/`
    pointing at a loopback registry served by `serve-vibestudio-registry.mjs`.
  - Details are in `HANDOFF-dolt-redesign.md`.
- **To ship:** the owner publishes the five packages, then the `file:`
  overrides and the registry knob are dropped.

### 3.5 Chronolog, `~/chronolog-db`

- **Main checkout:** branch `simplify-core`. Do not touch it.
- **`~/chronolog-worktrees/transport-iroh`**, branch `feature/transport-iroh`:
  the Iroh transport.
- **`~/chronolog-worktrees/celld-host`**, branch `feature/celld-host`, from
  `transport-iroh`: a Chronolog node as a celld deployment
  (`apps/chronolog-celld`). Handoff: `HANDOFF-celld-host.md`; its last update
  is committed as `4d00cee`.
- Nothing is pushed. The fork APIs it needed are merged into `unified-fork`:
  `dolt.execute(…, { log })`, `dolt.query` and `beginTransactionSync()`.

---

## 4. State per stream

### 4.1 The fork: done, with small open items

Everything in the status table of `docs/fork/design.md` is merged into
`unified-fork`.

- **Engine and Dolt cells:**
  - DoltLite as the only SQLite, with vendored patches: 0001 makes the stock
    engine the default; 0002 adds the deterministic context; 0003 adds
    `dolt_checkout --carry-ignored` and cheap checkout; 0004 adds packs.
  - Dolt cells, facets and fork.
  - F1–F7: merge with a resolver, commit in a transaction, local tables,
    cheap checkout, history reads, and remotes with grants, fetch, pull,
    fast-forward push and `ondoltpush`.
  - Deterministic execution for Chronolog.
- **Objects and secrets:** object fork, restore and import; per-class
  secrets.
- **Runtime features:**
  - P2P (Iroh) with cell-owned streams;
  - residency holds, where a code swap revokes holds at once;
  - a per-call CPU limit (`ctx.facets.get(name, startup, { limits })`);
  - codegen confinement, the egress hook, the vibestore bucket, the V8
    inspector, and the Windows port (compiles and cross-links).
- **Defect fixes:** S1–S4, plus E1, E3 and E5–E14.
  - E12/E13: a call ended by its CPU limit keeps its pending I/O, and the
    transaction it interrupted rolls back.
  - E14: the agents' persistent `transaction()` refusal.
- **Last full e2e** (release build of `75f1700`): 46 passed, 1 known failure
  (E2, the R2 etag), 2 skipped (`pi` and `opencode` need live model
  endpoints), 1 failed.
  - The failure is `python`: GET / times out with "Wasm code generation
    disallowed by embedder". It fails the same way on `2d1749d`.
  - Earlier full runs reported it as skipped, so it is probably a
    toolchain-dependent scenario now running for real.

**Open in the fork, in priority order:**

1. **`python` e2e.** Wasm codegen is refused for the Python example,
   probably by the fork's codegen confinement (`docs/fork/codegen.md`)
   applying to a worker that needs Wasm compilation. Decide the policy (Wasm
   compile is not `eval`), fix it, and make the scenario pass.
2. **Release artifacts for linux-arm64, darwin-arm64 and win32-x64.**
   Vibestudio's `targets.json` has nulls for them, and staging those targets
   is refused.
   - Fork side: build per target in CI; the Windows workflow exists, see
     `docs/fork/windows.md`.
   - Vibestudio side: `.github` workflows must place the binary in
     `native/celld/artifacts/<platform>-<arch>/` and fill in the digests.
   - See `HANDOFF-celld-runtime.md`, "What remains".
3. **The P2P carrier for Dolt remotes across workspaces.** The wire is
   specified and carrier-neutral (`transport`, `remote.serve`), but no
   cross-workspace run on two real nodes exists. P6 needs it.
4. **Known limits, documented and unfixed:**
   - A facet RPC in flight is not cancelled on abort; it rejects in
     JavaScript.
   - An unrelated event's failure while it drives an orphan block resets the
     object.
   - From §3b.17: a timer that re-arms a spin is ended once per limit,
     forever; a cut-off event's rollback also rolls back a concurrent event's
     transaction on the same object; and microtasks that V8 drops can belong
     to other objects.
   - From E14: the closing-checkpoint path of `finish_turn` was fixed by
     reading the code and has no scenario.
5. **Upstream defects still open:**
   - E2: the dev R2 etag is a counter.
   - E4: static-assets answers `/index.html` with a 307 and serves a 404
     with a JSON content type.
   - S4: a drain exceeds its release ceiling.
   - S5: a cell is released after a failed restore without proof of
     durability.
   - All four are in `upstream-findings.md`.
6. **Not pinned or measured:**
   - Checkout and merge benchmarks at GAD size, for the P5 gate.
   - Fork latency at 10,000 agent turns.

### 4.2 Vibestudio runtime cutover (`feat/celld-runtime`, contained in `feat/dolt-p0`)

- **Done:** Vibestudio runs on the fork instead of workerd, and the agent
  blockers C1 and C2 are fixed in the fork.
- **Remaining:**
  - the CI/release wiring for non-linux targets (§4.1 item 2);
  - V11, the Node surface, is exercised only by the test suites and lanes;
  - the owner's own instance has not been migrated.
- Host vitest has one known failure, `src/cli/agent/index.test.ts`
  (`auth.getConnectionInfo` has no `deviceId`); it predates this work.

### 4.3 The Vibestudio Dolt redesign (`feat/dolt-p0` + Base `feat/dolt-p2`)

`HANDOFF-dolt-redesign.md` (in `dolt-p0`) is the record. It has a status
table, commits, gate tables and lane tables for each pass. The phases are
those of `docs/dolt-redesign.md` §10.

| Phase                       | State                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| --------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| P0 Foundations              | **Done.** `@vibestudio/durable/dolt`: the typed `ctx.storage.dolt` surface, `committedTransaction` on F2, schema install commits, the schema-guard filter, framework tables local, `FakeDoltStorage`, and a real-fork probe.                                                                                                                                                                                                                                                                                            |
| P1 Runtime (fork)           | **Done** (F1–F7, the hard reset, `ondoltpush`).                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| P2a Residency               | **Done.** The eval heap is held until an explicit end, with revocation through `onrevoked`; the host lease is deleted.                                                                                                                                                                                                                                                                                                                                                                                                 |
| P2b General dynamic code    | **Done.**<br>• `hostCode` (`packages/durable/src/codeHost.ts`); the admission service `dynamicCodeService` (`admit`, `release`, `reset`, `dispose`); `EvalKernel` (Base `workers/eval-kernel`); `SessionDO` (Base `workers/session`).<br>• The CLI, panels, the system-test runner, the agent, the website spec and the headless test all pull.<br>• `evalService`, `EvalDO`, `UNSAFE_EVAL`, `resultReceiver`, redelivery, `evalEngine` and `settle` are deleted. The remaining `rg` hits are D5 migration code and deliberate guards. |
| P3 Durable services         | **Done.** `WorkspaceStateDO` is a Dolt cell, with a replaying import from the retired `WorkspaceDO`; the maintenance journal and `cloneDO` are deleted.                                                                                                                                                                                                                                                                                                                                                                |
| P4 Agent system             | **Mostly done**, see below.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| P5 GAD decentralized        | **Not started.**                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| P6 Chronolog capability     | **Not started.**                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |

**P4 in detail** (slices are in `docs/dolt-redesign.md` §6.8–§6.11):

- **S1, delete the hash chain: done.** The channel orders by
  `(sequence, envelopeId)`. GAD keeps its own private chain until P5.
- **S2, the Dolt engine for userland classes: done.**
  - The manifest declares `storage: "dolt"` and `doltLocal`.
  - A one-time, resumable SQLite→Dolt import exists.
  - `forkObjectStorage({ at })` and `cloneContext({ storageAt })`.
  - The channel is a Dolt cell: one commit per append, member change or fact
    change; the projections are local.
- **S3, the agent cell on Dolt: done.**
  - Pi uses `CompositeSqliteStorage`, and only conversation-knowledge
    document kinds are versioned.
  - One Dolt commit per turn, and only when a versioned table changed.
- **S4, native conversation fork across the channel and its agents: done.**
  The system scenario `conversation-fork-adopts-agent-cell` forks through
  the channel's own `fork` RPC, including edit-and-fork.
- **S5, subagent `mode: "fork"`: done.** Scenario
  `subagent-fork-inherits-context`.
- **S6a: done. S6b, the agent hosting its own eval kernel: half done and
  held.**
  - **Live on `feat/dolt-p0` (`7b8a6ddd3`, `db0e8c7dd`, `e3ca7401b`):** the
    host side.
    - The host recognises an eval host by a class manifest's `hosts`.
    - `dynamicCode.admit` hosts a run on the owner itself only when the
      owner asks for a persistent scope.
    - Finite scopes and `owner-session` targets stay session entities.
    - No class declares `hosts` yet, so the product still uses session
      entities, the designed fallback; nothing is lost.
  - **Built and held on branches:** the agent side, which adds `hosts` in
    the agent manifests, the vessel's relay to the kernel, and the eval tool
    addressing the owner. The branches are Base
    `feat/dolt-p2-s6b-agent-hosting` (`3553a91`) and host
    `feat/dolt-p0-s6b-agent-hosting`; Base's main line reverts it in
    `adc9993`.
  - **Why it is held:**
    - the kernel acts as the agent, so its calls to the agent (`agent.*`,
      `chat.*`, the outside-content report) were answered by its own client
      with "not exposed";
    - routing them out of the process hangs them at the host, and
      `agent-debug-state-method` and the harness's retire then hang too.
  - First suspect: a session entity carries `evalOrigin` (the owner), and an
    agent-hosted run loses it. The read-first list is in the handoff's "P4
    third pass".
- **Dolt compaction (gc): done** (host `a0663ac02`, Base `46b86c1`, kept
  through the revert).
  - Every Dolt class compacts on a geometric schedule: after 64 commits,
    then after half the commits it held at its last collection.
  - The check runs after a commit under `waitUntil`, in a block, and only
    over a clean working set. The agent checks after each closed turn;
    channels check every 8th commit.
  - On the real fork, a 19-commit cell is 0.66 MB after collection against
    5.4 MB uncollected. A collection changes no state, and a fork after it
    equals its parent.
  - Product lanes never reach 64 commits, so the agent's hook is tested at
    unit level only. `WorkspaceStateDO` is not yet on the schedule.
- **Bugs fixed on the way:** fork selection asked a headless `SessionDO` for
  `forkFrontier`, so a conversation fork from a CLI session had never worked;
  a forked subagent's adoption conflicted with its later open. Both are fixed
  in Base `f86483b`.
- **Input-gate refusals (E14):** fixed in the fork at `75f1700`, pinned in
  Vibestudio, and Pi's retry removed.
- **R1 measurements:**
  - Channel: 15.5 ms per event (p95 16.5), about two flushes, flat over
    10,000 events. 24 KB per event before gc, 12 KB after; gc takes 3.7 s at
    10,000 events.
  - Agent: 23 storage transactions and 1 commit per turn; turn mean 558 ms,
    p95 799 ms; a fork copy of 3.06 MB takes 247 ms.
- **Not measured or not exercised:** a product-level restart of an agent
  cell mid-run, and fork latency at 10,000 turns.

**The P4 wrap-up pass** (the handoff's "P4 third pass"; host head
`c8e2511a3`):

- **E14 in the product.** Fork `75f1700` is pinned (`6ab7464f5`), Pi's retry
  is removed (Pi `c6d518177`), and Pi `1.2.0-vibestudio.4-dolt.5` is pinned in
  the host, Base and five templates.
  - **Zero** `input gate` lines across three focused rounds and eight sweep
    groups.
  - `subagent-fork-inherits-context` and
    `conversation-fork-adopts-agent-cell` passed 3 of 3.
- **New open item, possibly the fork: `steer-a-running-child` passed 0 of
  3.**
  - Two runs timed out at 450 s, and in the third the headless cleanup timed
    out.
  - The cell hits `in_flight=64 limit=64`: 64 events in flight on one cell.
    That predates E14 and nobody has investigated it.
  - Find what keeps 64 events in flight on the agent's cell, and whether the
    fork should queue or the agent should not fan out.
- **The last lane sweep was stopped in `workers-b`**, so resilience, panels
  and subagents did not run on the final tree, and no failure was re-run.
  - Failures seen: `eval-exact-authority`,
    `channel-envelope-inspection-bounded`, `workspace-test-runner-extension`,
    `worker-env`, `dynamic-workspace-service`.
  - Each transcript shows a model or guest error, none in storage, fork,
    kernel or input gate.
  - `eval-exact-authority` failed 6 of 10 on the self-hosting tree and also
    fails on the final tree; that needs a look.
- **Gate:** green against the known baselines. The real-fork suites are 15
  files: 81 passed, 2 skipped.

### 4.4 Chronolog on the fork (`feature/celld-host`)

- **Works:** a two-node end-to-end test on the fork
  (`apps/chronolog-celld/scripts/e2e.ts`). It covers genesis, join,
  transactions on both sides with identical rows and identical Dolt commit
  hashes, a late transaction replayed into place, and a restart that
  resumes. Unit tests for `sqlite-celld`, `materializer-celld` and `rpc`
  pass.
- **The engine digest changed with fork patch 0003.** Any pinned Chronolog
  digest or fixture needs regenerating, and both nodes must run the same
  fork build.
- **Remaining**, from `HANDOFF-celld-host.md` (priority order):
  1. Port the conformance/replay fixture (`packages/conformance`) to drive a
     `CelldMaterializer` through a celld dev instance. Compare digests across
     two nodes and two TZ values.
  2. Simplify using the fork-polish fixes: delete `EventAnchor`; keep P2P
     streams in the object; `closed` settles; Ed25519 PKCS#8 v0 keys. The
     handoff update lists the call sites.
  3. Cutover deletions:
     - delete `apps/chronologd`, `materializer-doltlite`,
       `@dolthub/doltlite` and its pnpm patch, and the SSB network layer of
       `transport-ssb`;
     - move `feed-store.ts`, `types.ts` and `memory.ts` into a feeds
       package;
     - port the remaining chronologd features: snapshots and fork-seed,
       static membership, secret-store backends.
  4. node-core tests and a chaos harness against the celld host.
  5. Replay from the commit at the exact divergence point instead of the
     nearest `chronolog_cp_*` branch.
  6. Drop or keep the debug endpoint `/admin/authorized`.

---

## 5. The plan to finish the whole project (single-track, in order)

Each step ends with its gate green, the per-stream handoff updated, and a
commit. Vibestudio, the templates, Pi and Chronolog stay local; the owner
pushes them.

1. **Close P4.** Read `HANDOFF-dolt-redesign.md`, "P4 third pass", first.
   1. Re-run the lane groups the last sweep did not reach (resilience,
      panels, subagents). Re-run each failure once, and investigate
      `eval-exact-authority`.
   2. Investigate the `in_flight=64` saturation in `steer-a-running-child`.
      If the fork is at fault, fix it in the fork with an e2e repro (§6).
   3. Debug the S6b self-addressed-call hang (suspect: `evalOrigin` is lost
      for an agent-hosted run), then merge the two held branches; or record
      why the agent should keep a session entity.
   4. Run a model-free soak of an agent cell past 64 turns, so compaction
      runs in the product. Put `WorkspaceStateDO` on the compaction
      schedule.
   5. Add a product restart probe of an agent cell mid-run, and measure fork
      latency at 10,000 turns.
   - **Gate:** §10, row P4, of the redesign.
2. **P5, decentralized GAD** (about 20 agent-days, the largest phase). The
   design is `docs/dolt-redesign.md` §5.
   - A canonical Dolt database plus participant work cells (§5.1); contexts
     are branches.
   - Integration by F1 merge, `pull` and fast-forward `push` (§5.5).
   - Grants, and approval as grant issuance (§5.6).
   - Work-cell create and retire as an idempotent, crash-resumable protocol
     (§5.7).
   - A derived `commit_index` and `content_roots`.
   - Replay migration (Q2).
   - Delete the fact-map, radix and frontier machinery, and GAD's private
     hash chain left from S1.
   - **Gate:**
     - the provenance plan's acceptance matrix, re-expressed over commit
       hashes;
     - §5.13, the decentralization gates;
     - replay equivalence on real workspace snapshots;
     - the size gate;
     - checkout p95 within 20 ms with `fsync` and 10k local rows (the fork
       measured 14–16 ms).
   - Do it in vertical slices, like P4, and never leave two models in force.
3. **P6, the Chronolog capability.** Design: `docs/dolt-redesign.md` §8.
   Needs §4.1 item 3, the cross-workspace P2P carrier, and Chronolog step 6
   below.
   - A Chronolog node per workspace node and the `chronolog` host service.
   - Key custody, invite and join, and agent writes through approval (Q4).
   - Chronolog authority for pushes into a shared GAD `main`.
   - **Gate:** two workspace instances on two nodes. Create a group, invite,
     write from both sides, see an owner-row rejection, run a replay flip,
     and push GAD between workspaces authorized by group membership.
4. **Fork: the `python` e2e and the remaining limits** (§4.1 items 1 and 4).
   Can be done earlier if it blocks anything.
5. **Release.** This step goes before handing the result to the owner.
   - Build the fork for linux-arm64, darwin-arm64 and win32-x64.
   - Fill in Vibestudio's `targets.json` digests and wire the `.github`
     workflows.
   - Prepare the Pi packages for the owner to publish, and list the overrides
     to remove.
   - Prepare a merge plan of `feat/dolt-p0` (host) and the template branches
     onto their `main`s for the owner.
   - Delete the retired `WorkspaceDO` class and its import driver in the
     release after the one that ships P3 (see the handoff).
6. **Chronolog cleanup** (§4.4 items 1–6). Can be done before P6, which
   uses it.
7. **The pre-existing failures**, if the owner wants them fixed. They were
   present before this work and are not regressions:
   - host: `src/cli/agent/index.test.ts`;
   - Base: `vcs-skill-release-generator`, a stale content manifest;
   - system-testing: 4 failures in `cli.test.ts`;
   - Pi: 51 failures on its `main`;
   - lanes: `create-panel`, `workspace-test-runner-extension`, and
     `state-args-immediate-snapshot`, whose model script has a guest type
     error;
   - model-sample flakes: `call-do-method`, `eval-exact-authority`,
     `steer-a-running-child`;
   - `agent-debug-state-method`, an unexpected `channel.admin` prompt;
   - a rare ModelGatewayDO alarm hang, seen once in about 25 runs.

---

## 6. How to run things

**The fork**

```sh
cd /home/werg/celld
systemd-run --user --scope --quiet -p MemoryMax=6G -p MemorySwapMax=0 \
  env PATH=$HOME/.cargo/bin:$PATH CARGO_BUILD_JOBS=3 cargo build --release -p celld
CELLD_BIN=target/release/celld tests/e2e/run            # all scenarios, about 1-2 min
CELLD_BIN=target/release/celld tests/e2e/run continuations 'facet-*'
cp target/release/celld ~/celld-harness-bin/celld-uf-$(git rev-parse --short HEAD)
```

Expected: R2 etag XFAIL (E2); `pi` and `opencode` skipped; `python` fails
until §4.1 item 1 is fixed. Fork changes go on a `fix/*` or `feature/*`
branch. Each has an e2e scenario that fails before and passes after, an
E-entry in `upstream-findings.md` if it is a defect, a §3b section and a
status row in `design.md`, and the service doc. Then fast-forward into
`unified-fork`, push, and pin.

**Pinning a fork build in Vibestudio (`dolt-p0`)**

1. Put the commit and the linux-x64 sha256 in `native/celld/targets.json`.
2. Copy the binary to `native/celld/linux-x64/celld`.
3. Commit as `celld: pin fork <sha> (<what it fixes>)`.

The real-fork host suites use the staged binary; `VIBESTUDIO_CELLD_BIN`
overrides it.

**Vibestudio gate.** Exact commands are in `HANDOFF-dolt-redesign.md` and
`HANDOFF-celld-runtime.md`, "How to run".

- `pnpm type-check`, `pnpm lint`, and every `check:` script: authority,
  census, catalog, ledger, runtime-docs, agent-docs, `natlang:vendor`,
  userland-rpc-contracts.
- Package vitest, then host vitest (`vitest.host.config.ts`).
- The real-fork suites (the `*Celld.test.ts` files; run them with one
  worker).
- Base vitest, system-testing vitest, and Pi's tests for Pi changes.
- System-test lanes: eval-lifecycle, agentic-runtime, smoke, workers,
  harness-resilience, panels and subagents.
  - Run one test per invocation, on a fresh instance per group, with
    `VIBESTUDIO_TEMPLATE_CHECKOUTS` set to the symlink root.
  - Leave about 150 s between groups.
  - Re-run each failure once; when a failure looks new, run it on the
    pre-change commit.
- `--no-verify` is allowed only for the pre-existing `format:check`
  failure.

**Chronolog:** see `HANDOFF-celld-host.md`, "How to run".

---

## 7. Lessons that cost time

- **The fake hides engine behaviour.** Two bugs appeared only on the real
  Dolt engine:
  - primary-key columns report NOT NULL;
  - Dolt drops `--` comments from DDL, so the schema shape drifts at the
    second activation. Comments in Dolt-bound DDL are now refused.

  Always run a real-fork probe as well as `FakeDoltStorage`.
- **A probe that does not model the real shape proves nothing.** Three
  plain-object probes missed E14. The real shape was an event that had
  answered but was still polling ops handed to it on its way out. Read the
  runtime's state machine before writing probes.
- **Model-sample failures are common in lanes.** Re-run once, read the
  trajectory, and compare with the pre-change commit before calling a
  regression.
- **Old processes interfere.** A leftover lanes script or test sandbox
  (`bwrap … /tmp/vibestudio-test-run-*`, `node -e setInterval`) skews
  results and memory. Check `ps` before and after, and kill by PID.
- **Commit volume (R1).** Count transactions per call, not commits. Every
  transaction on a Dolt object costs about three flushes plus a replay
  nonce, and that, not commits, is what moves latency.
