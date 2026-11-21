# Handover: the unified celld fork, Vibestudio and Chronolog (2026-11-15)

This is the entry point for an agent that takes over the whole project. It
says what the project is and what the owner decided, where every piece of
work lives, what is done and how it was verified, and what is left, in order.
The detailed per-stream handoffs it points to are authoritative for their own
details; this file is authoritative for the overall order and the rules.

**Current release status, 2026-11-21.** Section 5 records the latest
release candidates and test evidence. The fresh-install and
two-workspace checks passed, including independent account writes and
reviewed publication into shared history. The 30-case agentic sample is
verified. Core deterministic cases, both desktop smokes, and all 43
browser cases passed. Android reached the onboarding panel and its
full smoke is still open. One managed test instance exceeded its
teardown deadline after its suites passed.
The 65-turn soak passed. The reserve guard stopped the separate
10,000-turn benchmark after it recorded at least 1,360 successful turns.
Section 5 tracks the outstanding platform builds and artifact pins.

**Additional test sweep, 2026-11-21.** The rebuilt celld passed all 53 regular
end-to-end scenarios, including the corrected R2 ETag contract. Pi and OpenCode
now have opt-in live checks. Pi reached its provider and received HTTP 401.
OpenCode submitted the prompt and then timed out waiting for its session.
The same stage timed out under stock workerd. A fresh rerun of
the three deterministic Testkit suites passed and stopped normally.

The next 100 agentic cases exclude the previous sample. The selection favors
private copies, reviewed publication, recovery, and build isolation. The first
pass exposed clean-history comparison, fixture seeding, typed move validation,
and context retirement issues. Base commits `c88ad54` and `51f79a8` passed 33
focused tests. System-testing `e000289` routes the two confirmed seeded VCS cases
through independent copies and removes the catalog's compile-time dependency
on the Personal browser runtime. All ten browser fixture inputs matched the
previous revision in a direct comparison. The live retry and remaining cases
use the exact pins recorded under
`~/.local/state/celld-p4-compaction/agentic-risk-100-20261121/`.
The six-case retry passed three, including both eval cases that had failed
while retiring their working copies. Base `79aa74b` then corrected comparisons
against an exact older revision and retained typed file-transfer errors. All
41 focused Base checks passed. System-testing `eb01855` gives 25 additional
seeded private cases their own copies before the model starts.

The next Testkit run passed all three suites and reproduced the shutdown
stall on channel recovery. It also found a tracked eval whose host had
already retired. Host `0383097d9` stops recovery before suspending the
workspace and closes admissions for retired eval hosts. All 71 focused Host
checks passed. The live retry stopped normally and recorded successful
cancellation of the tracked eval. The panel lifecycle check separately
reported an expected unload cancellation as a failure. Base `2bb467c`
preserves ownership of that cancellation through the RPC error wrappers.
All 99 focused Base checks passed. The isolated live panel recheck
`st_e56a3967b1724209afd5877312c13ca6` passed and its instance stopped normally.

The live agentic retry verified both VCS comparison cases, bringing the exact
passing count to five. The occupied-destination retry used scratch storage,
which allows replacement. System-testing `6371c2b` uses a managed repository
for that check. The managed rerun confirmed the error and both preserved notes.
System-testing `8199b42` clarifies that the agent must call the recovery helper
as part of the scenario. The first bulk run found that the publication case
could not see its seeded repository. System-testing `13083b6` gives the agent
a copy of that seeded revision and records the agent's exact publication event
for teardown. The isolated publication recheck
`st_079c036513c1409a8a64bfa15a06fd0d` passed with zero tool failures and completed
fixture cleanup. That brings the exact passing count to six. The rechecks
for panel lifecycle, occupied move, and fixture publication are in
`panel-fix-retry/`. The publication instance later stalled during shutdown
at startup artifact preparation. After the test result was retained, only
its disposable workspace process was terminated. The owner then retired
the instance. This stall remains open separately from eval cancellation.

System-testing `94f53e6` includes `dc47068` and lets private fixtures run
concurrently. Cases that exercise protected main run without concurrent
fixture snapshots. The fixture, publication, and scheduler checks passed
(83 focused tests, then 44 after the scheduler adjustment). System-testing
`572c59e` requires the published panel fork to have an observable publication
receipt (12 focused tests passed). Commit `e882aff` directs the worker
environment probe to the fixture's contract before it creates the worker.
The 93 outstanding cases and the occupied move rerun started together in
`bulk-resume/` as `st_6fac0271b3fb45f584d716788186d723`, with concurrency 3.
The occupied move rerun passed, including use of the executable recovery
helper. The exact result count comes from `summarize-sample.py` in the
evidence directory.

The first bulk cases exposed two large-history timeouts. Preparing the 36
revisions consumed most of the ten-minute scenario budget before the agent
started. System-testing `1904dc7` starts the scenario clock after repository
fixture preparation and retains preparation in the elapsed result. It passed
45 focused checks. Commit `e4add41` skips the private ancestry walk during
teardown when protected main still points at the original event, except where
the fixture needs exact creation-scope checks or has a publication receipt.
All 32 fixture checks passed. Commit `9efbc39` asks the revert scenario to
reread the restored file and verify cleanliness, the evidence required by its
existing checker.

The edited-import case completed its functional assertions and exposed a
native provenance bug when tracing a change to a tool invocation. Base
`3286e20` removes an extra empty field from the internal walk page. The
public provenance response still includes its own omission field. The
focused test passes with the host schema applied to both complete and
missing-metadata invocation pages. System-testing `2d4eef9` asks the
constraint-inference case to check the record of rejected alternatives. Its
agent reached the correct conclusion from ordinary reads in the first run.
The `history-retry/` batch rechecks these cases using the updated template
branches.

The coordinated run stopped normally when the disk fell to 2.3 GiB free.
Its retained progress contains 23 passing cases across all runs and 12
terminal failures. Retiring the disposable instance returned about 12 GiB.
`bulk-chunked/` holds the 65 unverified cases and the targeted multi-turn
rerun. It uses fresh, smaller groups and checks free disk space between
groups. System-testing `4942960` asks the persistence case to return both
eval observations. Commit `fee3255` reads the nested native error envelope
for the recoverable-infrastructure case. All 14 developer-ergonomics checks
passed.

The delegated-work cases found that review used the parent's stale projection
of the child's context. A child had committed a local change that the parent
could not inspect or integrate. Host `395fe2a27` and Base `66f8274` locate
the child's recorded authoring cell and copy its exact revision into the
authorized reviewer's own cell through the existing private fork grant.
The focused Host and Base suites passed 55 and 78 checks. The first
delegated-work rerun in `subagent-retry/` still read a seeded parent
revision. Base `ca22043` corrects the lookup to read the context ID from
the separate field in the stored participant identity. The exact-format
regression check is among 12 passing participant-history checks. The four
delegated-work cases are scheduled again in `subagent-author-retry/` after
the 66-case `bulk-chunked/` sweep. One design synthesis run also timed out
during recursive cleanup after the model finished.

The first history rerun `st_54b113d218054906a9a9f6f8c0cfedad` passed edited
import, edited-file history, walkable causality, and typed infrastructure
recovery. Its three retained trajectories exposed two other test evidence
issues and a repository history bug. System-testing `32d980d` accepts the
exact recorded invocation that authored the revert and joins the cited
historical work unit through its application to the exact commit. All 14
focused checks passed. Base `2376a51` includes repository coordinates in a
rejection walk. Its focused test keeps file lookups precise and excludes
other repositories. The three named live rechecks join the delegated-work
batch.

**Continuation, 2026-11-17.** P4 verification resumed in the feature worktrees.
Host `feat/dolt-p0` runtime changes are at `763409214`; Base P4 is `66683ea` (bounded
compaction inspection) and Base P2 is `66683ea` (promoted after the real-agent gate). System-testing P4 and P2 are `b64e74c` (65 real turns, normal fork and clean cleanup verified). Read "P4 continuation (2026-11-17)" in
`~/vibestudio-worktrees/dolt-p0/HANDOFF-dolt-redesign.md` before the older pass
records. The initial five defects were fixed and committed locally:

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
- Host `025cea4bd`: desktop and headless console history share filtering and
  cursor pagination. The headless producer previously returned a schema-invalid
  result for the correct `h.cdp.consoleHistory()` call. All 184 focused tests
  pass; `create-panel` now passes on the real stack.

The stopped sweep's five failures have each had their required rerun: four pass;
`worker-env` still has the known model descriptor-export error (the model then
recovered, observed the value and cleaned up). The two unfinished worker cases
are verified after the validator repairs. Harness resilience is **5/5 green**,
applicable panels **4/4**, and non-steering subagent/channel cases **3/3**.
BrowserImport remains inapplicable to this workspace. The validator suites pass
(91 and 25 tests); three captured executions pass revalidation; host/template
typechecks and lint pass. The full commit gate fails only the documented
formatting baseline in eight untouched host files.

**Further continuation.** Four additional defects were repaired locally:

- Host `d155d0093`: share only outstanding reads of the same canonical original
  envelope. Every ingress still verifies its own task and rechecks its owner;
  no result cache remains after settlement. The native trace proves burst
  pressure: 64 channel events arrived within 0.3–0.4 s and all later finished.
  The comparison had zero refusals and a child-channel peak of seven. All 35
  causal-proof tests pass; the fresh exact-authority case passes.
- Base `724b34e`: child retirement aborts and joins foreground and background
  tools. The previous flush path returned `retired:true` with a waiting tool
  still owned. All 93 focused tests and four Base typecheck configurations pass.
- System-testing `34b45b1`: preserve an expired test deadline across its own
  RPC cancellation wrappers, including synchronous-abort races; keep foreign
  cancellations and actual cleanup failures. All 46 focused tests and three
  template typecheck configurations pass.
- System-testing `8b0f329`: require a resolved file count and line total for the
  same set using the existing semantic claim checker. The old digit check
  accepted conflicting counts. Actual semantic replay accepts the retained
  resolved answer and rejects both conflicting-count answers.

**Strict steering passed on November 18.** Run
`st_4985fd0bd204444c833b08e4cb8d4474` returned 117 files and 22,510 lines,
explicitly correcting the initial count, and passed semantic validation
with zero tool failures. The agent took 85 s. The checker waited on a
first-use model credential approval until the known gateway received a
decision scoped to the disposable test session. The earlier strict case
`st_a584ecee9e6a428a9eb687289f0a4488` ended with a promise to report later and
returned no numbers (51 s, zero tool/refusal/cleanup failures). Earlier numeric
harness passes had unresolved count conflicts; do not call them correct task
answers. The separate `subagent-followup-after-report` case
`st_b5659669c216479bae7ea32f2653d597` reached its 420 s deadline with
`model.generation` and `suspend_turn` still pending. Its fresh run
`st_d540ac6d1f9b4ca2a466bc26925edc1d` passed in 260 s. Both November 18
runs had zero tool failures and no cleanup errors. The test instance and its
resource observer are stopped. Evidence is in `~/.local/state/celld-p4-compaction/`.
Retirement prepares in the fresh runs stayed below 0.6 s. This does not prove
that every archived slow prepare had the same cause.

**ModelGateway diagnostic.** Base `13085f3` exposes pending task state and open
connection count in its inspector (15 gateway tests and four typechecks pass).
An apparent gateway stall was a pending first-use credential approval, missed
when an earlier queue inspection happened after cancellation. In the isolated
verification instance, the known managed source received a session-scoped
credential-use decision; its native call then completed and the semantic replay
worked. No persistent credential grant or provider policy changed. A killed CLI
caller does not itself join a retained model turn: use its explicit cancellation
or retire the owned instance. Keep this evidence separate from older rare hangs.

**S6b is now complete (2026-11-17).** The held branches are merged locally.
Two independent RPC loopbacks caused the self-call failure: the hosted kernel's
request must leave its client, and the owner's reply to a distinct endpoint
with the same principal id must return through its incoming transport. Host
`11be2a5a0` restores hosted-client routing; `ddb4b356d` fixes replies, including
streaming and retirement terminals. No evalOrigin change or authority bypass
was needed. All 117 focused RPC/HTTP tests pass; the complete commit gate
passes through lint and reaches only the eight untouched formatting baselines.

Real-stack verification passes for actual `agent.describe()` and `chatOp`
self-calls (23 ms receiver calls), exact authority, live kernel continuity,
activation-loss replay without duplication, finite cancellation and forked
subagent inheritance. Each has zero tool failures; inspected trajectories have
no cleanup errors. The self-call group has zero overload refusals and zero
outstanding native events. One initial debug-state sample used `gad.inspectAgent`
and hit the existing channel.admin approval baseline; its required single rerun
exercised the self-call and passed. Full run ids and evidence are in the host
handoff's S6b completion record and `~/.local/state/celld-p4-s6b/`.

**Next:** the 10k-turn fork measurement, after the passed product compaction
soak recorded below. WorkspaceStateDO's
inherited common schedule passes on the real class (65 mutations, collection,
crash/restart, history and local-state preservation). The passed activation-loss
replay already provides the requested agent-cell restart probe: a fresh vessel
uses the same durable storage and completes once. Whole workspace-node restart
is outside that requirement. The strict
steering case now passes as recorded above. Continue remaining
work; a checkpoint or passing subset is not project completion. Capture native
cell DEBUG with both `RUST_LOG=info,celld::cell_event=debug` and
`VIBESTUDIO_LOG_LEVEL=verbose`, and confirm event lines before using them.

Development releases must use the canonical producer after
`NODE_ENV=development node build.mjs --source-server-prereqs`:
`scripts/prepare-development-workspace-release.mjs` with separate owned output
and scratch paths plus the explicit template checkout root. A plain build can
leave the source-generation pointer stale. The canonical producer validates
that pointer before compiling; startup also validates the completed release.
Do not reuse the earlier private preparation helper or bypass an input mismatch.

Completed verification instances, observers and release producers have been stopped
and joined. Their private release/cache scratch was removed. Bounded evidence
is at `~/.local/state/celld-p4-steering/` and the individual system-test artifacts.
The owned npm registry was replaced after joining the preceding fixture and remains live for P4 (node PID 472729,
timeout parent 472728; verify the command before stopping), and must be joined
at final cleanup. The fork runtime and binary pin remain `75f1700`; only handover
docs changed in this repo. Nothing pushed; main checkouts remain untouched.

**Memory and compaction diagnostic (2026-11-17).** The latest isolated run
`st_ee67575c9ef94e20a62a41cd864e54db` retains 65 distinct native turns, the
64-commit collection marker, and zero model calls. It is still **red**:
critical mission retirement and subsequent context cleanup failed. Host
`2659eaa6f` fixes the reproduced critical-confirmation principal error (81
focused tests pass; full host gate reaches only the same eight formatting
baselines). The SQLite fault is now located at the first schema read during
`release blob retentions`, but its competing lock owner remains unproven.
Do not raise waits or treat the native turn evidence as a passing cleanup gate.

Host `763409214` initializes the shared retention registry in WAL mode before
native producers start. A traced diagnostic identifies the native writer as
the conflicting lock owner, and reproduces exhaustion at namespace DELETE as
well as the earlier schema read. The root-reader regression fails before the
change; all 12 interop/retention/GC/node tests now pass, including 65 real Dolt
writes retained across durable shutdown and restart. Full host checks pass
through lint and reach only the eight formatting baselines. The fresh untraced
actual-agent soak passes and P2 is promoted; the 10k-turn fork measurement remains
outstanding. No native binary, wait duration, retry policy or deadline changed.

**65-turn gate passed:** `st_444e20c9d29f4ffb8847b8ce350b4753`, 205.9 s,
65 distinct native receipts, zero model calls, collection at the default 64
commits, and no mission/session/context cleanup errors. Base and System-testing
P2 branches are fast-forwarded to the verified P4 heads above. The scope peaked
at 3.22 GiB, had no MemoryMax/OOM events, and all its processes are joined. The
complete comparison release was removed. Proof and memory summaries are
`soak7-workload-evidence.json` and `soak7-memory-summary.json` in the private
compaction state directory. Continue with the 10k native-turn fork measurement;
later redesign phases are still open.

The extended short gate also passes: `st_ba15f3a72fc14833af2ebd3783c5df5e`,
223.8 s, including an ordinary channel fork after 65 native turns, clone
adoption/history/model checks, and explicit clone/session/context retirement.
Fork RPC: 9,895 ms; exported native facets: 35,008,303 bytes. Its scope peaked
at 3.91 GiB with no MemoryMax/OOM events; all processes are joined. System-testing
P2 is now `b64e74c`. The complete comparison release is held for the fresh
`p4-agent-10k` instance, not abandoned. The explicit-only benchmark has a
12-hour investigation bound and sparse bounded progress/memory evidence.
Native TypeScript checks separately peaked at 5.84 GiB; keep all heavy phases
serial under 6 GiB and zero swap. Continue monitoring and completing the
measurement. The later redesign phases remain open.

The long benchmark `st_508a958252c04cd2a2ed577f117b6df0` confirmed 6,784
native turns before the kernel killed `celld` in its 6 GiB group at 07:21 CET
on November 18, about 10 hours 45 minutes into the run. The fork was never
measured. The managed instance removed its temporary workspace, so the
inspect and trajectory commands could not retrieve the run. The kernel
record, progress receipts and memory samples are under
`~/.local/state/celld-p4-compaction/`. The 65-turn fork remains green.

Two local fork fixes followed. `380b152` makes a loaded Dolt facet inherit
the remote declaration from its storage root. The Vibestudio relay probe
failed before the fix, then passed with the target observing the pushed
head and rows. The full fork suite matched its baseline of 46 passes, one
known R2 failure, two skips, and the existing Python failure. `21688dc`
makes Linux pressure handling read the nested group that owns the memory
limit. The old runtime returned null readings inside the 6 GiB scope.
The repaired runtime reports the group charge and enters and clears pressure
mode as a small controlled allocation is made and released. The long fork
benchmark remains open. On the final host `25cc0ece9`, Base `bb4bd05` and
System-testing `b64e74c`, the 65-turn case
`st_da87a35ae59540f985a26795a751a317` passed with collection at 64, zero
model calls, a 7.4-second fork and clean retirement. The host and Base
scheduling fix counts commits while a check is queued. The fresh long
comparison `st_d70af7029dba4c5db94c709a569cbd8e` used the same 6 GiB cap
and a 24-hour test budget. It crossed pressure thresholds and recovered from
several shedding episodes, then the kernel killed `celld` after 960 verified
turns. The final allocation spike was anonymous memory, with almost all
file cache already reclaimed. The 10,000-turn fork still has no result.
Evidence and owned process state are in `~/.local/state/celld-p4-compaction/`.

Dolt capture now streams LTX output from the input pages and reads only
headers on an ordinary reopen. In a separate 96 MiB Dolt collection,
peak anonymous memory fell from 827 MiB to 644 MiB under the same cap.
The storage regression verifies byte-for-byte snapshot compatibility.
All four focused Dolt end-to-end scenarios pass. On native build `1ea8681`,
`st_b75930306d734129ae39111a0555377d` passed 65 native turns with collection
at 64, zero model calls, a 10.2-second fork, and clean retirement.
The long model-free measurement `st_171e03fc4df7457eab8afc6b2f0faf5a` used
the same build and 6 GiB cap. Asking Linux to reclaim 1 GiB of the test
group's file cache cleared a sustained admission stall and four queued
activations. At 12:16 CET a separate native allocation spike killed the
runtime after 1,216 validated turns. The kernel recorded 6,067,793,920
bytes of anonymous memory and 245,760 bytes of file cache at the kill.
The managed test and observers are joined. The fork was never reached.
The private state directory has the intervention and terminal evidence.

A diagnostic rerun captured repeated buffer growth during Dolt handoff
snapshots, including a reallocation from 176 MiB to 352 MiB while a 275 MiB
input was held. The diagnostic is stopped and its local allocator patch is
out of the source tree. The handoff now reads Dolt input in chunks and
publishes from a temporary file. An isolated 96 MiB fixture published a
97 MiB VibeStore snapshot, restored all 96 rows after removing local state,
and reduced publication's peak anonymous memory from 399 MiB to 178 MiB.
Build `497f018` passes all eight LTX tests, four focused Dolt scenarios,
and socket hibernation. Its product check
`st_a6fb72de38b14fdf85dc466100f768a9` passed 65 turns, collected at 64,
made zero model calls, forked in 11.5 seconds and retired cleanly. The
full-size measurement is now running on the same build as
`st_55ddfbaa2da24973ac95f24de722772f` in `p4-handoff-stream10k`, with the
6 GiB cap and a 24-hour test budget. At 576 validated turns, file cache
held the pressure latch and four cold activations waited. A scoped
1 GiB reclaim cleared the latch and unblocked those activations. The
test scope also uses a 5 GiB soft reclaim threshold from that point.
The 6 GiB hard cap and zero swap stayed in force. The owner then prioritized
participant working copies. We stopped this disposable benchmark after the
1,216-turn checkpoint with zero recorded kills and joined its instance.
The matching-source run `st_e1fef31aa554491797d8bbb1940893bd` started on
November 19 in `p4-current-fork-benchmark2`. It is the same serial,
model-free 10,000-turn case with a 24-hour test budget. Its managed scope
has a 10 GiB hard ceiling and zero swap, with the desktop outside it.
The source launcher matched the release prepared from Host `e52812f61`,
Base `0fcaf2b`, and celld `b78e4d0`. The owned runner joins and retires
the benchmark and registry after a terminal result. The memory sampler
and outcome use the prefix `p4-current-fork-benchmark2-` in the private
state directory. This run ended on November 20. Memory pressure preceded
a missed node-lease renewal, and the first celld process stopped itself
when its authority expired. The kernel then killed its replacement at
the scope's 10 GiB limit. Its anonymous RSS was about 8.6 GiB at that
point, after the scope's file cache had been reclaimed. The run has no
terminal benchmark result, and
the separate 65-turn result remains the verified functional evidence.
The prepared-release and installed pair checks were relaunched serially
as `vs-verify-combined-installed-workspace.service` once the benchmark
instance and registry had stopped.
The isolated P5 worktrees
are `base-gad-participants` and `gad-participant-admission`. The first
participant fork and conflict-merge probe passes against the real runtime.

**Private content and publication, November 19.** Base `e8a090c` resolves
file content through the caller's admitted Dolt branch or the exact protected
published event. Host `1a72f98b6` applies this check to raw blob reads and to
manifests that include private file content. The authenticated uploader can
read its own unimported bytes.

In managed run `runtime-source38`, the signed author read a private file,
an unimported upload and their tree. Another signed session could not access
the four exact byte digests or the typed tree. After an exact one-file
publication approval, the second session read the published file, retained
its previous working revision and still could not read the unimported upload.
The run used the held replay migration and the packaged `c8dfc2e` runtime.
It stopped normally with a 4.47 GiB peak and zero recorded memory-limit
or OOM events. Host checks passed 180 tests, targeted lint and type checks.
Base checks passed 226 tests and all four type configurations. One separate
repeat of the Base compiler check was killed by systemd-oomd during user
slice reclaim at 5.8 GiB. The evidence is in
`p5-managed-runtime-source38-evidence.log` and
`p5-native-raw-blob-final-host.log` under the private state directory.

Base `097a1bd` and host `1107c6dbc` reconstruct private file scopes before
attaching a frozen replay. They include earlier revisions of private files
and exclude bytes already in the migrated public history. A retry resumes
registration safely, and a durable receipt prevents repeated scans after
startup. In managed run `runtime-source39`, a signed participant wrote a file
in the legacy workspace, reopened the exact context and working revision
after replay, and read its original bytes. A second signed participant could
read neither the file nor its raw digest. The run peaked at 3.33 GiB, recorded
zero memory-limit or OOM events, and stopped normally. The 137 focused host
and 226 Base tests and both type checks passed. A separate Base type check was
killed at a smaller memory cap. The rerun passed all four configurations
with no other heavy build running. See
`p5-managed-runtime-source39-evidence.log`,
`p5-native-replay-private-content3.log` and
`p5-replay-private-base-types2.log`.

Host `09175c1e9` can recover ownership of a migrated interactive copy
from durable authenticated session records on ordinary startup. It
assigns ownership only when those records identify a single user and
keeps any recorded owner. In managed run `runtime-source40`, the frozen
history was installed and stopped with the original copy still unassigned.
After a normal restart, its original signed owner reopened the same
context and read the same bytes. A second signed session was refused.
Both launches stopped normally. They peaked at 3.39 and 4.59 GiB.
The 66 focused host tests, type check and targeted lint passed. This
verifies recovery of installed history. First-time cutover remains open.
See `p5-managed-runtime-source40-evidence.log` and
`p5-replayed-session-owners-check2.log`.

Base `0271c6d` and host `be3759b67` bind newly produced workspace
tree metadata to its exact semantic placement. Publishing the same
metadata through the protected event makes it readable without rewriting
the underlying objects. Managed run `runtime-source41` renamed a file
whose bytes were already published. Its author could read the raw and
typed tree. A second signed participant could read the published file
bytes and was refused the private tree. After the exact rename approval,
the second participant could read the renamed path and retained its
independent working revision. The run peaked at 4.55 GiB, recorded zero
memory-limit or OOM events, and stopped normally. The 158 focused host
tests, type check and targeted lint passed. See
`p5-managed-runtime-source41-evidence.log` and
`p5-exact-tree-placement-host.log`.
Existing unclassified manifests made before this change still require
scope recovery where their private paths cannot be inferred from file
bytes.

Base `3fed20a` records the resolved intent with new native work units.
Historical rows without these fields still derive intent from their
original evidence. In managed run `runtime-source42`, on host
`6db18495e` and the pinned `a6dce5d` runtime, a signed participant
read the exact intent and traced file blame to that work unit. The same
run passed private rename isolation, approved publication and preservation
of a second participant's working revision. It stopped normally with a
4.32 GiB peak and zero memory-limit or OOM events. All 227 focused Base
tests and all four type configurations passed. See
`p5-managed-runtime-source42-evidence.log` and
`p5-native-intent-projection-types.log`.

Base `50400c4` and host `46b685b92` run workspace search over the
admitted working revisions and the exact protected main event. The
trajectory mirror searches messages reached through command invocation
coordinates from those same revisions. In managed run `runtime-source43`,
the signed author found uncommitted work. A second participant could not
include that work in their search even when naming the author's context.
After the exact publication approval, their search found the work and
their independent working revision stayed in place. The run stopped
normally with a 4.55 GiB peak and zero OOM events. The 107 focused host
tests, 229 Base tests, both type checks, and targeted lint passed. The
search tests also cover older intent rows and exact cross-channel
trajectory visibility. See `p5-managed-runtime-source43-evidence.log`,
`p5-native-search-check.log` and `p5-native-search-check2.log`.

Base `7ee4720` and host `43bb9894f` attach read-memory to the exact
participant revision and returned content hash. Authored episodes and
saved events come from native history. Recorded command invocation
coordinates resolve through the existing trajectory mirror. In managed
run `runtime-source44` on the pinned `82a5781` runtime, the signed author
read their private authoring intent and file history and received `stale`
for a different hash. The reader received `unmanaged` before publication
and until their own explicit merge. After merging, the reader recovered
the exact original work and saved event. The instance stopped normally
with a 4.20 GiB peak and zero memory limit or OOM events. The 83 focused
host tests, 39 Base tests, host and four Base type checks, and targeted
host lint passed. See `p5-managed-runtime-source44-evidence.log` and
`p5-native-file-memory-check.log`.

Base `b992db2` and host `092c6c0c6` now walk causes, cohorts and
rejections through admitted native revisions. Managed run
`runtime-source46` verified stable cause pagination, the exact cohort
save, rejection of a private work-unit probe, and the attempted intent
and applied change behind an undo. It used the same prepared release as
`runtime-source45`, whose review was still preparing when the fixture's
approval deadline expired. The rerun adjusted the fixture deadline and polling interval.
It stopped normally at 4.33 GiB with zero memory-limit or OOM events.
See `p5-managed-runtime-source46-evidence.log` and the focused host and
Base results in `p5-native-history-walk-check.log`.

Base `0e418e4` and host `89b29b4c5` run provenance queries on the same
admitted native revisions. The worker supplies the approved views to
the fork's `queryScoped` boundary. SQLite authorizes underlying reads
only through those views. Fork `634d297` preserves aggregate queries by
materializing the trusted projections when SQLite requires it. Managed
run `runtime-source48` verified the author's private intent and exact
application basis, result limits, admitted file placement, aggregate
queries, and refusal of direct backing-table reads and unrelated private
work. After publication, the reader queried the exact event and work
membership while retaining their own working revision. The run used
the locally built `634d297` runtime, peaked at 4.61 GiB without OOM or
memory-limit events, and stopped normally. The 83 focused host tests,
55 Base tests, targeted lint, and host and four Base type checks passed.
See `p5-managed-runtime-source48-evidence.log`,
`p5-native-provenance-query-types2.log` and
`celld-scoped-read-count-acceptance.log`.

Host `6cd55892e` resumes native history startup after a system-owned
canonical lifetime has been reserved. Managed run `runtime-source49`
stopped with the legacy source unfrozen and no replay receipt. A normal
restart froze the protected event, replayed the original working copy,
reopened the same context for its authenticated owner and refused access
from a second signed participant. Both starts stopped normally. The restart
peaked at 4.71 GiB with no memory-limit or OOM events. Base `74439cc`
shares the external-edit draft planner with the existing history path.
See `p5-managed-runtime-source49-evidence.log`
and `p5-external-delta-planner-check.log`.

Base `2fadc1c` and host `1864afa50` verify scoped history reads before
freezing a workspace. Managed run `runtime-source50` reserved the canonical
lifetime, restarted on the older pinned runtime and made another write
to the still-active legacy working copy. A second restart on `b78e4d0`
migrated the later private revision and reopened it for its signed owner.
A second participant was refused. The three scopes stopped normally, with
a peak of 4.69 GiB and no OOM or hard-limit events. See
`p5-managed-runtime-source50-evidence.log`.

Base `7745744` and host `ab52ca3bd` stage sealed external changes in the
owner's native history. Managed run `runtime-source52` verified that the
working revision, file contents and shared history stayed fixed while the
owner registered and superseded a draft. Retrying the original registration
returned its exact receipt. An independent signed participant was refused
both the draft and its raw blob. The instance stopped normally with a peak
of 3.03 GiB and no memory limit or OOM events. See
`p5-managed-runtime-source52-evidence.log` and the matching release manifest.

Host `8d0105869` and Base `e85b9b9` complete the live draft route. In managed
run `runtime-source58` the writer reviewed the actual old upstream content,
withdrew one draft, and applied another that combined independent line edits
and required an explicit choice for a conflict. It refused early finalization,
replayed the exact merge and finalization receipts, and saved the external
source on the resulting event. A second signed participant was refused the
withdrawn draft and its raw blob. After approval of the exact two-file diff,
the second participant read the merged files from shared history and kept
their independent working revision. The instance stopped normally at 4.22
GiB peak, with zero memory limit or OOM events. See
`p5-managed-runtime-source58-evidence.log` and its release manifest.
Host `8f6d5035f` pins all four staged binaries from the successful
`8c0898f` artifact run `44539697971`. The later runtime with exact
Chronolog snapshot reads passed Linux and Windows artifact builds, and the
macOS job was cancelled before it received a runner on both `macos-26` and
`macos-15`. The targeted macOS rerun later completed on `macos-15`.
Host `f5cbd9d82` pins the resulting four-platform `9116b07` release.

Host `c2569708b` routes ordinary context clones through the originating
participant's working history. Managed run `runtime-source59` used the
four-platform pin's Linux binary. The signed creator saw the private file at
the same revision in the clone, a retry returned the same clone, and an
unrelated participant was refused. The shared head stayed fixed. It stopped
normally at 3.64 GiB peak with zero memory-limit or OOM events. See
`p5-managed-runtime-source59-evidence.log` and its release manifest.
Host `f0ec59d87` and Base `6077d1a` then retained the captured source for
the admitted clone lifetime. In managed run `runtime-source60`, the cloned
Durable Object called normal VCS methods under its own authenticated identity
and read the captured private file. The source still reported its original
revision, the clone retry returned the same result, and an unrelated caller
was refused. The shared head stayed fixed. The instance stopped normally at
3.78 GiB peak with no memory-limit or OOM events. See
`p5-managed-runtime-source60-evidence.log` and the release manifest.

Host `d72055f91` reserves native history after activating a workspace with
a manifest that declares the native history class. The first normal startup
then found a legacy code lookup from an infrastructure context. Host
`9eef46137` and Base `2d61bed` now restore a storage fork from its source's
sealed executable. Managed run `runtime-source62` used normal provisioning
and the staged `8c0898f` Linux binary. It opened two signed working copies,
applied and finalized reviewed external changes, and published the exact
two-file diff. The second participant read the published files while their
private revision stayed fixed. The instance stopped normally at 4.88 GiB
peak with zero memory-limit or OOM events. See
`p5-managed-runtime-source62-evidence.log` and the matching release manifest.
Host `e9922b2f6` also routes foreign-epoch publication through the native
protected gate. Its focused host tests pass. The installed cross-epoch
restart still needs a live run.

**Native checkout measurement.** The replayed workspace had 5,135 versioned
files and 10,000 local table rows. Each warm run had 200 quiet checkouts and
200 after a local write, alternating between distinct revisions. Collecting
the carried table names directly from the captured source catalog avoids
building and storing two temporary catalogs. The two optimized runs measured
13.03 ms native p95 when quiet and 19.73–19.95 ms after a local write. Before
the change those ranges were 14.60–14.79 ms and 21.41–21.48 ms. The optimized
completed-request p95 was 42.61–43.29 ms and 50.14–50.77 ms respectively.
A separate syscall trace confirmed successful disk syncs during both cases.
The trace and the uninstrumented request samples are in
`p5-native-checkout-profile/` in the private state directory.
The E19 Linux candidate measured 29.10 ms native p95 while quiet and
40.29 ms after a local write on the same 10,000-row fixture with the
long fork benchmark running. The earlier optimized `a6dce5d` Linux
artifact measured 31.61 ms and 41.14 ms under the concurrent load.
Both exceeded the 20 ms target. A separate E19 trace recorded 114 and
232 successful sync calls. The live reference comparison means this
run does not isolate an E19 regression. The queued
`vs-verify-e19-release-checkout.service` verifies the exact Ubuntu 22.04
artifact's commit and digest, then runs the latency check and sync trace
after the benchmark and installed checks finish. See
`e19-current.json`, `e19-sync.json` and
`loaded-reference-a6dce5d.json` in the same directory.

An independent replay of the same template-sized workspace exported
4,067,212 bytes and forked in 219 ms. It verified one committed event and
three working contexts across 135 repositories. See
`p5-actual-replay-verify3.log` for the recorded size and copy time.

The native disjoint-merge fixture used the same 5,135 files, 135
repositories and 10,000 local rows. Two branches each changed 16 distinct
file placements. Each of 200 warm requests per condition verified that
all 32 changes survived and retained the same source branch and local
row count. Request p95 was 66.03 ms when quiet and 67.79 ms after a local
write. Those timings include the request and the native merge commit.
The fixture excludes host review and approval time. See
`p5-native-merge-profile/latency.log`.

The optimized `a6dce5d` Linux release passed the full model-free sweep:
48 passed, the known R2 ETag contract failed as expected, and the two
model-dependent scenarios were skipped. The container scenario passed in
the 8 GiB verification scope. It needs capacity for five declared 1 GiB
reservations plus the runtime. The earlier 3 GiB run refused a container
under admission pressure. See `celld-optimized-release-full.log` and its
JSON report. The same revision passed Windows recovery in GitHub Actions
run 44511291186 and built all four target artifacts in run 44511291136.
Host `6db18495e` stages and pins those four downloaded binaries after
verifying each artifact's commit and SHA-256. The exact pinned Linux
binary passed 48 scenarios with the expected R2 failure and two model
skips. The queue restore test now uses a two-second scan interval in its
30-second assertion window. With the production default of 60 seconds,
a restored queue whose alarm starts in the next minute may wait past
that assertion. The test also verifies the restored message's delay.
The successful exact-artifact run peaked at 511 MiB with zero OOM
or limit events. See `celld-a6-ci-wake-full.log` and
`host-pin-a6-artifact-staging.log`.

P5 has a local implementation on Base `feat/gad-participant-cells` and
host `feat/gad-participant-admission`. Base `69ffa6b` and host `ccf995f0b`
use native pin `60f0bf8`.
The real runtime forked two independent GAD facets, refused a stale
publication, resolved the conflict in the second work cell and kept both
independent file changes on shared main. A third cell fetched and merged
a published revision without moving main. An approval binds both heads
at the native remote boundary, including when the same token is used
through a different carrier. The participant can derive its saved event
from its working revision, archive that saved event across a retry, and
revoke the participant's grants. The host's self-fork reservation fixes
its source commit across an interrupted copy and repeated requests.
The fork's two Dolt remote scenarios and 13 remote unit tests, 24 focused
Base tests, 188 focused host tests and three native hosted tests pass.
Host and Base type checks pass.

A read-only backup of a fresh managed workspace held one event,
three working contexts and 5,135 files across 135 repositories. Replay
verified every placement and located each context tip. The new Dolt
facet exported 4,067,212 bytes. A native fork at the restored head took
219 ms and the clone returned the original head and row counts. The
separate multi-branch fixture proves restart-safe replay and preservation
of both parents. The editor still routes through the SQLite semantic
workspace. Continue through the existing workspace-source interface with
host-authenticated participant identity and the hosted self-fork path.
Bind the semantic planner, materialization effects and existing host
approval to participant history, and finish lifecycle wiring through
physical destruction. The real replay fixtures and evidence are in the
private state directory.

The participant storage now resolves public event and application IDs against
the requested context's ancestry, including both parents of a merge. The
existing provenance serializer writes the prepared edit and its placement
changes in one Dolt transaction. The native probe verifies rollback, exact
command retry, rejection of a reused command with a different input, and a
separate saved event. The host keeps content and effect acknowledgements with
the owner that started the command. The focused checks passed 25 Base tests,
59 host orchestration and publication tests, and both type checks.
Placement content roots can be rebuilt from the retained history. The native
test checks that the participant's roots include its private edit and the
shared history's roots do not. The incremental index and integration into
host content collection are pending.

The migration now has an explicit fast-forward installation step for an
empty destination. A real replay was installed twice, forked in 237 ms from a
4,278,036-byte export, and projected all 5,135 original file placements in
135 repositories. The test also rejects installing over a later destination
commit.

The manifest now admits `GadHistoryDO` as the hosted Dolt history class. The
legacy owner exports ordered, individually verified replay steps. Each hosted
step records the exact input and compares the full placement proof before
committing. Repeating a completed step returns its original commit. The
host can derive a participant from its verified task or user coordinates, and
the class records the exact native fork commit before requesting allocation.
Only the allocated participant can use its remote grant through the class's
relay. The class exposes context reads, prepared edits, saved revisions,
publication and retirement. Retirement fences new work, archives saved
revisions, revokes grants and requests the existing runtime's deletion.
An approved push grant is bound to the exact previous and proposed main
commits and expires after five minutes. The focused test verifies that an
expired grant leaves main unchanged.

The disposable native probe used the real class and the replayed 5,135-file
workspace. Two copies retained the whole starting tree and retried their
original allocations. A grant presented by the other copy was rejected.
One copy published its saved revision while the other stayed at its initial
head. Retirement retained the second copy's saved revision and resumed from
the archival receipt after an injected deletion failure. This probe used a
fixture for host approval, allocation and physical deletion. It verified
the class and native storage, with the existing UniversalDO fork and host
retirement covered separately. The four new host tests, 25 focused Base
tests, and both type checks pass.

The managed system workspace passed its startup doctor. Normal runtime
admission registered `GadHistoryDO`, but the first call reached a 404 on the
UniversalDO engine check. The shared durable base now dispatches the existing
root-authenticated `__legacy` route. A fresh managed instance admitted the
class and reached application authority, where an ordinary session was
correctly refused access to participant allocation. The corrected scope
peaked at 4.66 GiB with zero memory-limit or OOM events. See
`p5-managed-hosted-fixed-admission.log` in the private state directory.
Its graceful stop stalled while draining the extension host. The disposable
supervisor and workspace process were stopped by verified PID, and their
reapers completed retirement. The three real-celld facet tests for fresh
storage, fork and SQLite migration passed after teardown.

An authenticated probe in the managed host replayed the template workspace
and recovered four original context tips. Allocation initially failed on a
missing `__lifecycle/initializeClone` handler. The durable base now binds
that call to host authority and the exact clone witness, commits versioned
initialization with its local receipt, and returns the same result on retry.
The rerun opened two copies through normal runtime allocation, retried one
allocation, and retired it while the other copy stayed readable. The probe
used a temporary test entry point and the ordinary host dispatcher. Its
scope peaked at 5.50 GiB with zero OOM events and shut down normally. The
36 durable and dispatch tests and three channel fork tests pass. See
`p5-managed-replay-fixed-evidence.log` in the private state directory.

The existing edit planner now reads placement through an exact revision
interface. The Dolt participant resolves the public state reference to the
expected branch commit before reading its coordinates without a checkout.
The real-engine fixture used the planner to edit both copies of the 5,135-file
workspace and refused an unrelated state. It published one saved revision
and archived the other. Its host approval, allocation and deletion
substitutions are unchanged. The 19 focused Base tests and all four Base
typecheck profiles pass. See `p5-native-edit-plan.log` in the private state
directory.

The participant now requests exact text content before planning, rejects
bytes with a different digest, and prepares a complete placement and
provenance plan using the shared semantic assembler. Snapshot and applied
content origins come from the participant's own Dolt ancestry. The native
probe used the class to compile and persist both edits, verified the
preservation edge and recovered both origins, and staged a file creation
without advancing the working branch. See `p5-history-complete-edit-plan.log`.

The host dispatcher now uses the existing blob store and its collection
lock to persist a participant's content before the application commits.
The participant recovers the exact prior edit result from its versioned
command and application records after a lost reply. The managed edit
probe used this host adapter to create a private file, checked the exact
edit retry, and confirmed that the second copy and canonical history
did not have the file. Its scope peaked at 5.50 GiB with zero OOM events
and shut down normally. The 64 focused host tests and both type checks
pass. See `p5-managed-edit-evidence.log` in the private state directory.

The host projects two owners of the same context into distinct folders.
Each folder keeps the original public context binding. Projection repair
uses the participant's current native revision, catches deleted or changed
working bytes, and advances intact folders by the changed repositories.
Saves derive the exact application sequence from the working branch and
recover the same receipt on retry. Pending integration decisions still
require their own semantic validation.

The managed probe exercised both folders, removed a disposable file and
recovered it, saved the editing participant's revision, and retired that
cell while the other stayed readable. The scope peaked at 5.55 GiB with
zero OOM events and shut down normally. The projection and context
materializer tests and both type checks pass. See
`p5-managed-projection-evidence.log`.

The host stages approval through the existing protected-ref gate before
requesting the exact native push. Base `d471917` and host `def182828`
retain the approved previous and proposed revisions, recover confirmed
pushes after a lost reply, and project them in shared-history order.
The native fixture rejected a mismatched approved event and recovered the
confirmed revision. The focused host gate passed 77 tests.

Base `96fcbf8` brings a clean copy forward to an exact shared revision
and previews a divergent copy without moving its branch. A prepared
whole-row resolution writes the decision and application in the native
merge transaction. Save verifies that every integration has its exact
two native parents. On the captured 5,135-file workspace, Bob kept his
file mode, saved the merge, published it to shared main, and Alice pulled
that result into her copy. The fixture confirmed the merge and save
receipts on retry and rejected an omitted resolution without advancing the
branch. The four Base typecheck profiles and 44 focused tests passed.
See `p5-native-published-integration.log` and
`p5-accounted-merge-base-check.log` in the private state directory.

Base `df939c3` and host `a7fd6104a` derive status and exact file reads
from the admitted participant's revision. The native fixture checked
status before and after publication and integration, original and current
file modes, selector order and repository boundaries. Independent
placements merged automatically without decision rows and kept their
source authoring lineage. The host checks the file coordinates and bytes
against the requested revision and digest. Host and Base typechecks and
67 host and 44 Base tests passed. See
`p5-native-participant-file-read.log` and
`p5-participant-file-read-check.log`.

Base `3c39515` plans whole-row conflicts from the selected revisions and
typed merge request. The native probe selected each side, refused a
missing choice, recovered an exact retry, and rejected changed input.
It also proved that a second participant cannot read an unpublished
application but can read the published event while its branch is behind.
The content collector enumerates live and retiring cells and reads
canonical roots after their private roots. Host `30d4198eb` rechecks
those roots under the existing collection lock and recovers confirmed
publications when the participant source is attached. Native GC queries
saw a new blob only in its author's cell until publication. The host
collection test retained earlier private content, newly written content,
and content transferred to canonical history during retirement. Host and
Base typechecks, 75 host tests and 8 Base participant tests passed.
See `p5-native-history-gc-roots.log` and `p5-history-gc-check.log`.

Base `52aec77` keeps the discard intent and receipt in the participant's
local storage. After an interruption following the native reset, the
next operation completes the receipt before accepting new work. A later
retry returns that result and leaves newer work intact. Host `88cc536d9`
repairs the folder from the current participant revision on retry.
The native discard probe, both typechecks, 56 host tests and eight
participant tests passed. See `p5-native-history-discard.log`.

Base `5650347` composes disjoint edits to the same text placement and
records exact content mappings to both parents. Host `cc0bbc679` reads
the selected bytes, stores the composed content under its verified
digest and keeps it protected until the merge commits. The native
fixture composed separate line edits, rejected incorrect observed
content and an incorrect storage receipt, and checked both content
edges and exact retry. Both typechecks, 71 host tests and 44 Base tests
passed. See `p5-native-history-text-merge.log`.

Base `e8882cc` and host `1c1bbb184` merge a reviewed committed
revision from a published participant context through the same
content handoff. The native fixture refused to fetch the unpublished
branch, then merged and saved it without changing shared main. The same
run retained a mode change while composing text and kept the explicitly
selected local file when both copies created a file at the same path.
The unresolved collision left the branch intact. Host and Base checks
passed with 63 and 44 tests. See `p5-native-peer-integration.log`.

Base `9f580ec` and host `8d596b207` provide exact participant
repository lookup and bounded file and directory pages. Directory and
file lineage comes from the selected Dolt revision. The native fixture
listed a saved private integration, checked both files from its merged
parents and refused a cursor reused with a different prefix. Host and
Base typechecks and 8 host and 39 Base tests passed. See
`p5-native-participant-catalog.log`.

Base `278c79e` shares the move and copy planners with the existing
semantic store. The runtime fixture copied across a merged parent's
exact revision, retained the content lineage through a rename and moved
a repository without changing its files or identity. Host `d8135817f`
binds normal filesystem calls to the admitted copy when the participant
source is attached. Its tests kept the copied roots and handles separate
for two callers of the same logical context. Both typechecks, 143 host
tests and 50 Base tests passed.

Disposable instance `p5-hosted-fs-1118` replayed its installed
workspace, attached the participant filesystem source, and exercised
ordinary filesystem write, read, copy and rename with two host test
callers in one logical context. The second copy and main kept their
original content. The scope peaked at 5.50 GiB with zero memory-limit
events and zero OOM kills. Shutdown stalled while an automation agent's
retirement tried to reserve a participant. The verified inner disposable
workspace process was terminated, its supervisor completed cleanup,
and the temporary registry and source hook were removed. See
`p5-managed-filesystem-evidence.log` and its memory and stop receipts.

Base `83f1634` reuses the established allocation and its ordinary
grant. Binding an already-open branch needs no new remote fetch. Host
`02375bcf3` uses an exact source lookup for single-file rename and
prevents new participant admission after shutdown starts. The native
probe verified one ordinary grant and one allocation per participant
across retries. The second managed instance completed the same
filesystem probe in 60 seconds after the doctor trigger and stopped
normally. Its scope peaked at 5.50 GiB with no memory-limit events or
OOM kills. The hook and registry were removed. Both typechecks, 145 host
tests and eight Base tests passed. See
`p5-managed-filesystem-fixed-evidence.log`.

Host `9b9f14fee` routes ordinary file and repository reads, edits, moves,
copies, saves, discard and protected publication through the admitted
participant while that history is attached. The service checks exact
references against the participant's visible history. Base `98d5cf2`
resolves an exact repository identity without retrieving file bytes.
Both typechecks, 187 host tests and eight Base participant tests passed.
The native fixture verified the repository lookup and inaccessible
unpublished references. See `p5-public-vcs-check.log` and
`p5-public-catalog-native.log`.

Base `64dfc1b` and host `8e23bfbc5` prepare filesystem consent for
publication and discard from the two exact native placements. The
native probe verified both sides of a file rename, both repository
paths after a move and an empty diff for duplicate revisions. The
checks passed with 102 host tests, eight Base tests and both typechecks.
See `p5-public-consent-native.log` and `p5-public-consent-check.log`.

A paired CLI verified protected publication through the regular public
VCS service in disposable instance `p5-managed-public-1118`. A temporary
host hook replayed the workspace and attached its native history. Alice's
task used the ordinary VCS routes to edit, read and save one repository.
Bob saw the old main while approval was pending and after the first
request timed out and its approval was cancelled. On retry the paired
device granted one approval for the exact one-file diff. The publication
returned the saved event, the same command retried with the same
receipt, and a separate session read the newly published bytes from
shared history. The first cold approval preparation reached the
180-second test deadline. Warm preparation finished in under a second.
That test required separate one-time context approvals for native
storage allocation. The test scope peaked at 5.50 GiB, recorded no
hard-limit or OOM events, and shut down normally. The temporary hook and
registry were removed. See `p5-publication-alice-retry.log`,
`p5-publication-bob-after.log`, and `p5-managed-public-stop.log`.

Base `4c77475` keeps the storage entity in the history provider's runtime
context while the participant's admitted logical context still selects
its separate working branch. Host `5ddba9be8` fixes inherited-context
forks by reserving and activating them in the same resolved context.
Host `0194f197f` admits an exact discard retry by its private receipt
after the discarded state is gone. In fresh disposable instance
`p5-managed-first-use-fixed-1118`, the paired CLI created its copy
without an extra approval, edited, discarded, edited again and retried
the original discard. The retry returned the same receipt, preserved
the new edit and left main unchanged. The instance stopped normally
with a 5.40 GiB peak and zero memory-limit or OOM events. Both
typechecks, the native interruption fixture, and the focused host
suites passed. See `p5-managed-first-use-fixed-cli.log`,
`p5-public-retry-check.log`, and `p5-inherited-runtime-fork-check.log`.

Base `5e378e2` and host `54a3d2fba` resolve a reviewed shared event and
the caller's exact target state to native revisions. A merge into an
ancestor records a working application and two parents without adding a
decision for a clean adoption. The merge returns the exact application,
authored and incorporated change counts, and any recorded decisions.
The native fixture rejected an unpublished saved event as shared main
and verified that retry after a later save returns the same application.
Host and Base typechecks, 99 host tests and eight Base tests passed. See
`p5-native-merge-receipt.log` and `p5-native-merge-receipt-check.log`.

Base `e6ee8a7`, `6503a1e` and host `1904860c2` compare a
participant's current working state with the exact current shared event.
The native preview reports placements, per-side authoring evidence,
conflicts, and text combinations whose bytes were verified. It
rolls back the preview and binds the page cursor to both inputs.
The comparison uses the same intent-evidence rules as the existing
history. Both typechecks, 100 host tests and 11 Base tests passed.
The native fixture verified a two-page conflict review and an observed
text composition. See `p5-native-reviewed-compare-2.log` and
`p5-public-compare-check-fixed.log`.

Base `d67956f` and host `cf8ceaf1c` route the admitted public merge
command through the reviewed native operation. The receipt shows
adoption and explicit decisions from the original application. A clean
merge omits its decision ID. Website file consent selects paths from the
same exact native revisions without reading content during preparation.
The native fixture verified these paths, clean receipts, composed-text
receipts and exact retry. Both typechecks, 117 host tests and 11 Base
tests passed. See `p5-native-public-merge-2.log` and
`p5-public-merge-check.log`.

Disposable instance `p5-managed-divergent-1118` passed the paired
conflict-and-publication case through the public CLI. The temporary
host hook replayed and attached the 5,135-file workspace. Two separate
tasks edited and saved different text at the same file. Alice's exact
one-file publication received a once approval. Bob's saved copy kept
his text, and comparison identified the conflicting content and its
three exact digests. Bob kept his text in an explicit merge, saved the
two-parent result, recovered the same merge application on retry, and
published with a separate once approval. A third task read the exact
published bytes. Bob's polling for the incoming change filled the CLI
journal limit, so the evidence for later steps is the completed script's
assertions, paired approvals and the independent reader. The instance
stopped normally at a 5.50 GiB peak, with no hard-limit or OOM events.
The hook and registry were removed. See `p5-managed-divergent-resume.log`,
`p5-managed-divergent-bob.log`, and `p5-managed-divergent-stop.log`.

Base `afa866d` records the completed replay and each exact migrated
context tip in local canonical storage. It verifies saved ancestry
before installing and rejects a changed retry. Host `08cb22a2b` verifies
the recorded tips and uses them on reattachment. The native fixture
read all three migrated tips after shared main advanced, recovered the
original installation result without moving main, and rejected changing
its context set. Both typechecks, 68 host tests and eight Base tests
passed. See `p5-native-replay-receipt.log` and
`p5-public-replay-receipt-check.log`.

Base `ceec94b` and host `8145e64e6` archive exactly the storage
owners anchored to a retired context before reclaiming them. A failed
archive keeps the projection intact and stops new admissions to that
context. Retrying removal resumes after entity retirement. The native
fixture interrupted after archival, recovered the same targets, and
left other participants readable. In `p5-managed-retirement-1118` a
paired CLI saved an unpublished file. Ordinary session detach retained
the context because its hosted kernel was still live. A signed call to
`runtime.destroyContext` joined that kernel, verified the exact private
content digest in the archive and reclaimed the retired cell through
the host's native storage manager. A second context remained readable
and main stayed unchanged. Both typechecks, 225 host tests and eight
Base tests passed. The disposable instance stopped normally at 5.50 GiB
with zero hard-limit and OOM events. Temporary source and registry were
removed. See `p5-native-context-retire-3.log`,
`p5-managed-retirement-archive-proof.log`, and
`p5-managed-retirement-reader-after.log`.

Base `20320fe` and host `872c69cfb` inspect work units and commands
through the participant's visible native history. The fixture verified
the exact command-to-work-to-application relationships, bounded edge
previews, and intent derived from recorded effects. The other participant
could read the same command only after it was published. Host and Base
typechecks, 44 host tests and 38 Base tests passed. See
`p5-native-provenance-inspect-2.log` and
`p5-provenance-inspect-check.log`.

Base `e25baa9` and host `4a3eb850d` also inspect saved events and
working applications. Native projections report the actual revision
and preserve the public fact-root representation for legacy responses.
Exact bound native parent queries keep both parents visible after a
different cell fetches shared main. The native fixture verified saved
parents, application order, applied file effects and the merged event
from the other participant. Both typechecks, 76 host and package tests,
and 38 Base tests passed. See `p5-native-provenance-state-2.log` and
`p5-provenance-states-check.log`.

Base `c397bb7` and host `e489f21f2` inspect authored and applied
changes at the admitted native tip. The fixture verified the underlying
effects, state parent links in both directions, and both content sources
of an automatic text merge. Host and Base typechecks, 76 host and package
tests, and 38 Base tests passed. See `p5-native-change-edges.log` and
`p5-change-graph-check-exact.log`.

Fork `f9b25fb` repairs DoltLite historical reads for tables with implicit
clustered primary keys. The host's stock SQLite default had caused the
historical schema reader to decode committed rows using a different layout
from their writer. A minimal native reproduction returned blank values
from a key-only table. The runtime regression test verifies both key-only
rows and rows whose primary-key order differs from declared column order.
The disposable fixture with the new runtime verified that the saved text
merge decision identifies its actual source, target, recorded outcome,
resulting change, and source authorship. Base `e7702ad` and host
`464d6e861` expose and validate the decision. The release build, eight
runtime tests, both typechecks, 76 host and package tests, and 38 Base
tests passed. The pinned test binary is `celld-uf-f9b25fb` with SHA-256
`61b299f63ea44f85ead170d1fd6b965851711b351974d447577d4d62f8674dc5`.
See `p5-versioned-schema-rust94.log`, `p5-native-decision-fixed.log`,
and `p5-decision-graph-check.log`.

Base `0e41ed5` and host `57f7323df` inspect a file or repository at its
exact working or saved revision. The native test verified stable file and
repository IDs after moves, the reverse edge from a copied file to work
that copied it, and prior state IDs for saved file and repository
deletions. It also followed the decision back from the incoming authored
change. Both typechecks, 76 host and package tests, and 38 Base tests
passed. See `p5-native-placement-provenance-2.log` and
`p5-placement-provenance-check.log`.

Host `fa969593a`, `4a6551f76`, and `c32b703b7` resolve mirror
trees and context-file execution through the authenticated participant's
working history. A device's distinct owning sessions have separate copies.
Contexts reached through the same owning session can share its storage
and retain independent branches. Signed agent tasks retain their sealed
task identity. The first managed attempt found an allocation collision
between two sessions on one device. The rerun used ordinary signed
filesystem writes, mirror reads, and CLI execution. One session read and
ran its private script. The second saw its original tree and was refused
the first session's tree hash and script path. The host typecheck and
focused suites of 83, 108, and 122 tests passed. The managed instance
stopped normally at 5.50 GiB with zero hard-limit and OOM events. See
`p5-managed-mirror-eval2-evidence.log`, `p5-managed-mirror-eval2-mirror.log`,
and `p5-participant-device-session-fixed.log`.

Base `6c08eef` and host `2da787faa` page adjacent history links through
the participant's admitted revision. The native fixture checked that
concatenated pages reproduce the inspection order and that a cursor
fails when reused on a different root or visible revision. Both
typechecks, 76 host and package tests, and 38 Base tests passed. See
`p5-native-neighbors.log` and `p5-neighbor-pages-check.log`.

Base `26a49bf` and host `fe0447d7d` trace content annotations from
the selected participant revision back to both authors of a text
merge. Where both parents carry the same unchanged span, the
annotation follows the applied base once and keeps both recorded
parent mappings. The native fixture verified imported snapshot
origins, mode-only changes, and paged, disjoint merge annotations.
Both typechecks, 76 host and package tests, and 40 Base tests passed.
See `p5-native-blame-authors.log` and `p5-native-blame-check.log`.

Base `0aa63ff` and host `1d165ef49` page event history at the
participant's admitted revisions. The native fixture verified that
both saved parents are reachable, a future traversal from shared
history excludes private events of a different participant until
publication, and cursors cannot change direction or roots. Both
typechecks, 76 host and package tests, and 38 Base tests passed.
See `p5-native-event-timeline.log` and `p5-event-timeline-check.log`.

Base `4ab1f0b` and host `f4e57beb5` page authored file history
at an exact revision. The native fixture verified clean adoption,
explicitly accepted changes with the exact decision ID, retention of
local changes, and exclusion of declined source changes. The first
run surfaced that a whole-file choice can retain source history
without a local applied-change row. The implementation now reads the
persisted adoption itself. Both typechecks, 76 host and package
tests, and 38 Base tests passed. See `p5-native-file-timeline-final.log`
and `p5-file-timeline-check.log`.

Host `494a388b3` and `f726d8623` carry a verified caller's
working revision into runtime image preparation. Byte-identical
source can reuse one build key while each signed revision keeps a
separate stored execution digest. The first private Durable Object
image does not install the class's default image, and its RPC
authority reads use the sealed execution digest. The host
typecheck and suites of 64 build and 264 runtime tests passed. See
`p5-exact-build-check.log` and `p5-runtime-exact-source-final.log`.
Reserved activation, explicit unbound references, and declaration
or development builds still need their cutover review.

The signed launch on a disposable workspace found a recursive
allocation. The storage fork that creates a participant cell tried to
read source from the same cell. Host `4a5e7a50d` keeps that physical
fork off the participant source path. A retry confirmed that a signed
edit and ordinary worker creation used the same private application,
that the retained execution matched its immutable build artifact, and
that another signed session saw the original worker source. The
test workspace stopped normally with no OOM kills. See
`p5-managed-runtime-source4-evidence.log` and its shutdown log.

Base `8b6f3be` and host `9ba79442b` require a reachable workspace
revision before ordinary artifact-key reads disclose build metadata,
inventory or bytes. In a fresh signed run, the writer retrieved its
build and the second session was given the exact key. The second
session saw neither the build metadata nor its artifact inventory.
Both typechecks, 152 host and package tests, and 38 Base tests passed.
See `p5-build-visibility-check.log` and
`p5-managed-runtime-source5-evidence.log`.

Host `ef8c03238` resolves ordinary `ctx:` builds under the signed caller's
context and binds both the immutable content and the exact semantic revision.
The signed run on the pinned CI Linux binary confirmed that the compiled worker
contained the private edit and that a second signed session could neither build
from that context nor look up its build. See
`p5-managed-runtime-source7-evidence.log`.

Base `0a78534` and host `142fb2a24` read template installations
from the exact saved canonical event. The native fixture found the
original installation after publication of a two-parent integration.
The focused suites and both typechecks passed. See
`p5-native-template-installation.log` and
`p5-template-installation-check.log`.

Base `a2397ce` and host `b825bd180` can pause legacy edits at an exact
published event before migration. A retry with the same operation ID resumes
at the same frontier. Migration verifies the replay receipt, including the
private context revisions, and keeps the pause until explicitly released. The
two typechecks, 78 host tests and 38 Base tests passed. The disposable managed
run refused a legacy context creation during the pause, released it for the
diagnostic host's remaining legacy lifecycle, and passed the two signed private
build checks. That scope peaked at 5.50 GiB, recorded zero OOM kills, and
shut down normally. See `p5-history-freeze-check.log`,
`p5-managed-runtime-source8-evidence.log` and the managed shutdown log.

Base `04c91f5` persists the participant identity entitled to restore a
migrated private context. Host `e133958b9` requires the recorded owner
to match the signed caller and reads it again after host restart. Contexts
without a verified owner stay in retained replay storage. A regular
participant with the same context name starts from shared main. The
managed run observed the saved owner on reread and refused reassignment.

Host `45243396c` checks access to an execution's exact source before
reporting its retention record or content roots. Multiple executions of
the same compiled bytes retain separate inspection results. With two signed
sessions, the writer inspected its own execution and the reader received the
same response for its digest as for an unknown digest. The host and template
typechecks, 182 host tests and 38 Base tests passed. See
`p5-replay-owner-check.log`, `p5-replay-owner-base.log` and
`p5-managed-runtime-source10-evidence.log`. This run stopped normally with
zero recorded OOM kills.

Base `d2434bc` and host `106255809` preserve the authoring command, work
unit, and changes behind a replayed external delta. The native Linux
probe staged a generated-template update and returned the same delta
and change ID at the published event. The prior event had no access to
that delta. Both typechecks, 46 host and 29 Base tests passed. See
`p5-external-history-check.log` and
`p5-native-external-history-fixture/verify.log`.

Host `e0794b442` applies the same participant-history permission check to
ordinary builds and runtime launches that name raw content. In signed
managed instance `p5-managed-runtime-source12-1118`, the writer built and
launched from its private source hash and recovered the original exact
execution. The second session saw the unedited version and was refused
build, launch, and execution access with the exact private identifiers.
See `p5-raw-build-final.log` and
`p5-managed-runtime-source12-evidence.log`. The earlier source11 run was
killed by `systemd-oomd` after loading the workspace and cold-building three
templates in one scope. The source12 run completed after preparing the
templates separately. Its managed scope peaked at 4.73 GiB and recorded
no memory limit or OOM events. Its registry and instance were stopped.

Base `29532f1` captures a private fork at its first chosen revision. It
transfers the revision directly with a grant for the destination storage
owner and removes the temporary branch and grant after acknowledgment.
Host `267db5ae5` uses this procedure for an authenticated semantic-only
fork. The native fixture on the pinned Linux binary verified separate
copies, receipt retries after the parent advanced and after the grant was
revoked, an independent child edit, and publication of the child's saved
revision. An unrelated copy stayed at shared main. The fixture supplied
its own carrier and approval receipt. The 227 focused host tests and
eight Base tests passed. See `p5-context-fork-small-checks.log` and
`p5-native-scoped-context-fork/verify.log`.

Host `9b054bc5f` and `b50383244` recognize the verified relationship
between an active eval executor and its owner when forking or editing an
owned child context. The signed CLI run `p5-managed-runtime-source15-1118`
forked the original private edit, changed both branches independently,
retried the fork without replacing the child's work, and kept the working
revisions private from a second session. The 212 focused host tests
passed. The managed scope peaked at 3.31 GiB with no memory-limit or OOM
events and shut down normally. See `p5-signed-fork-authority.log` and
`p5-managed-runtime-source15-evidence.log`.

Base `b1fb374` and `4810363` and host `d62dc5241` durably close
an owned child branch while its parent stays active. The native fixture
retained the saved child revision, dropped an unsaved edit, and retried
successfully after a lost archive reply. The signed CLI saved and closed
the child and verified the parent still had its later working revision.
The original signed run exposed the engine's 128-character grant label
limit for real participant identities. The repeat passed with a bounded
label, peaked at 3.51 GiB, recorded zero OOM events, and shut down normally.
The 261 host tests, eight Base tests, and both typechecks passed. See
`p5-native-context-branch-retirement/verify.log`,
`p5-fork-retirement-typechecks.log`, and
`p5-managed-runtime-source17-evidence.log`.

Base `5a9d065` and host `b27bc8ba7` route exact file and history
reads to the branch that contains the requested reference inside the
admitted caller's context graph. A signed writer read its child's
private file, and the second signed session received `InvalidReference`
when given that same application and file coordinate. The probe's
wrapper expected different error text. The signed CLI result records
the refusal and neither side reported an OOM kill. See
`p5-managed-runtime-source18-{writer,reader}.log`. The native fixture,
103 focused host tests, and both typechecks passed.

Base `438b03e` and host `7da011364` keep build-source receipts in the
participant cell and recheck branch reachability when using them. The
host reconstructs the requested bytes and checks their digest. The
native fixture recovered the exact private receipt after restarting the
node. A separate signed writer advanced its workspace, then compiled and
launched the earlier exact source. The second signed participant was
refused access with the exact hash. The 89 focused host tests and both
typechecks passed. The signed test peaked at 3.73 GiB, recorded zero OOM
events, and shut down normally. See
`p5-native-build-source-receipt/verify.log` and
`p5-managed-runtime-source19-evidence.log`.

Base `f294999` and host `6edbd0f61` route a signed build to
the caller's owned child copy by context or exact content hash. The
writer compiled both ways and inspected the resulting execution. The
second participant was refused both build paths and execution
inspection. The 96 focused host tests and both typechecks passed. The
managed run peaked at 4.95 GiB with zero OOM kills and shut down
normally. See `p5-child-build-typechecks.log` and
`p5-managed-runtime-source20-evidence.log`.

Host `9fb19ace6` recovers outstanding native publications before
checking that the canonical event matches protected shared history.
A mismatch keeps the attachment inactive. Base `4482b61` and host
`03bbf74e8` bind the installed replay to the original write freeze.
The disposable installed host verified that the same freeze resumes
without staging any revision and that releasing and reacquiring the
same operation cannot reuse that receipt. The source refused legacy
writes during the freeze. The 68 host tests, 226 Base tests and both
typechecks passed. The managed run peaked at 3.35 GiB with zero OOM
kills and stopped normally. See `p5-frozen-plan-check2.log`,
`p5-frozen-base-tests.log`, and `p5-managed-runtime-source23-setup.log`.

Host `4e077f5b5` resolves worker listings, service declarations, and
live documentation from the caller's admitted copy. Build preparation
and resolution use the exact source that supplied the declarations.
A signed writer saw its unpublished worker title, service, and docs.
The second participant saw none of the three. The 66 focused tests
and host typechecks passed. The managed run peaked at 3.39 GiB with
zero OOM kills and shut down normally. See
`p5-private-catalog-check.log` and `p5-managed-runtime-source24-evidence.log`.

Host `fdb0e292e` and `1eb13088a` verify retained execution
descriptors at the handoff to owner publication. A signed session had
failed to start after another participant published because the build
bytes survived but the old execution descriptor had not been admitted
for that write. The 203 focused tests and host typechecks passed. Host
`7a27b0449` keeps the signed person's working copy bound to the same
interactive session across separate requests. The 141 focused tests and
host typechecks passed.

The signed end-to-end run verified a private edit across distinct runs
of the same session. Alice then published from an owned child copy.
Bob's separately saved work remained private as shared history moved.
His reviewed integration retained the selected text, recorded Alice's
event as a parent, and gave the same application on retry. A third
session read Bob's exact published event from shared history. The
managed workspace peaked at 3.78 GiB with zero OOM kills and stopped
normally. See `p5-execution-publication-proof.log`,
`p5-owned-interactive-session-check.log`, and
`p5-managed-runtime-source28-evidence.log`.

Base `f57bdc2` and host `539b59330` select saved revisions from
the caller's owned branches for compare and merge. The signed writer
compared its private child, merged it into the parent, saved the exact
child event as a parent, and retried the same command. Shared main
stayed at its previous revision. A second signed participant was
refused compare and merge even with the child's exact event. The 114
host tests, 226 Base tests and both typechecks passed. The managed
run peaked at 3.77 GiB with zero OOM kills and shut down normally.
See `p5-owned-branch-merge-check2.log` and
`p5-managed-runtime-source29-evidence.log`.

Base `d7d9fd0` and host `1e2e16c90` copy from an admitted private
branch and retain the exact source file and revision in provenance.
The signed writer copied bytes from its saved child, verified that
lineage and retried the same command. Shared main stayed at the
previous revision and the second participant could not copy the
private file. The 113 host tests, 226 Base tests and both typechecks
passed. The managed run peaked at 3.64 GiB with zero OOM kills and
shut down normally. See `p5-owned-copy-check.log` and
`p5-managed-runtime-source30-evidence.log`.

Base `3c4712f` saves a complete snapshot in an independent working copy
through one branch advance. The native probe verified replacement by
stable file identity, exact external provenance, isolation from the other
copy and shared main, and the same receipt after a process restart.
Host `b23bec141` requires bytes independently supplied by the verified
producer for a native import. Knowing or retaining the digest does not
prove an upload. One signed session uploaded, imported in a separate run,
read the exact bytes, and retried the same command. A second signed
session could not import by that digest. The 177 host tests, 226 Base
tests and both typechecks passed. The managed instance peaked at 3.74 GiB,
recorded zero OOM kills, and stopped normally. See
`p5-native-import-snapshot/verify3.log`, `p5-host-snapshot-upload-check3.log`,
and `p5-managed-runtime-source31-evidence.log`. The general blobstore's
digest-only reads still need participant visibility enforcement.

Base `b8a7a12` and host `99c277452` apply undo against the caller's
exact participant history. The native fixture reversed a text edit
through its recorded content mapping, restored file and repository
tombstones, and verified an identical retry. The signed workspace run
restored text and an entire project with the original file identity.
Shared history stayed fixed, and the second signed session could not
counteract the first session's private change. The 48 focused host
tests, 226 Base tests and both typechecks passed. The managed workspace
peaked at 3.50 GiB, recorded zero OOM kills, and stopped normally.
See `p5-native-revert/verify3.log`, `p5-native-revert-check2.log`, and
`p5-managed-runtime-source32-evidence.log`.

Base `d19dbc0` and host `b673142d5` register, list, and retire ordinary
runtime contexts in the installed history. The native probe verified
literal prefix matching and retirement across restart. The managed
workspace kept the original legacy freeze held while it registered and
retired a context. Two signed sessions then opened independently and ran
the private text and project undo workflow. The 230 host tests, 226
Base tests and both typechecks passed. The managed workspace peaked at
3.64 GiB, recorded zero OOM kills, and stopped normally. See
`p5-native-context-lifecycle/verify.log`,
`p5-native-context-lifecycle-check.log`, and
`p5-managed-runtime-source33-evidence.log`.

Base `4f4f658` and host `ae877b6a8` and `03c27c494` resume installed
participant history at ordinary startup. The host verifies a system-owned
canonical context, the original held migration and the protected head.
In the persistent test workspace, startup reattached to the same
installation with the test trigger absent. The signed writer re-opened
the exact saved event and bytes. The separate reader saw the same shared
revision and was refused the writer's event. The 73 focused host tests,
226 Base tests and both typechecks passed. Startup and restart peaked at
3.38 and 4.77 GiB, recorded zero OOM kills, and stopped normally. See
`p5-native-history-boot-owner-check.log` and
`p5-managed-runtime-source35-evidence.log`.

First-time installation still needs to move to ordinary startup.
The remaining public semantic operations are open. Incremental peer
prefixes and remaining structural choices still need their cutover
checks. Direct native fixtures use synthetic approval receipts.
The managed probes enter through a temporary host-only hook and use the
ordinary dispatcher.

The real replay also supplied a checkout benchmark with 10,000 local rows
and two distinct committed file states. At the runtime's default NORMAL
synchronization setting, 200 HTTP turns each measured p95 of 45.64 ms for
quiet switches and 52.11 ms after a local write. Read-only control p95 was
1.34 ms. Batching 100 switches in one turn took 11.13 ms per switch on
average. The in-turn clock held steady, so this gives no in-turn p95. The
hosted checkout path exceeds the 20 ms target. See
`p5-actual-checkout-isolation.log` in the private state directory.

The preceding diagnostic instance is stopped/joined. Its whole scope peaked at 3.66 GiB,
then fell below 2.85 GiB, with no quota hits or OOM kills. Native RSS peaked at
1.59 GiB and settled around 1.1–1.2 GiB. This is substantial overhead, not a
proven accumulating leak. Heavy verification is strictly serial, each scope
has MemoryMax=6G/MemorySwapMax=0, and its bounded observer runs outside it.
The owner’s other applications/processes remain untouched. Keep private release
output only through the immediate corrected comparison; join exact owned
processes before retiring their scratch. Bounded evidence lives under
`~/.local/state/celld-p4-compaction/`.

Read in this order: §1 (goal and decisions), §2 (rules), §3 (map of repos),
§4 (state per stream), §5 (the plan to finish), §6 (how to run things),
§7 (lessons).

---

## 1. Goal and the owner's decisions

On November 20 the owner removed backward compatibility and additional
migration work from release scope. The release checks target a fresh
workspace on the current runtime and a restart on that same runtime.
Existing workspace data and earlier migration evidence are retained.
The release gate covers private saves, normal restart, reviewed publication
and merging between participants. Forced server crashes and lost replies are
optional follow-up coverage.

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
  - Q2 originally selected full replay. The November 20 release direction
    supersedes that migration requirement.
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

- **Branch:** `unified-fork`, with local runtime fixes through `21688dc`.
  `75f1700` is the pushed runtime revision.
- **Rust:** 1.94.1 at `~/.cargo/bin`; the system rustc 1.93 is too old.
- **Docs:**
  - `docs/fork/design.md`: status table, single-track backlog, and §3b,
    the decisions as built (§3b.1–§3b.18).
  - `docs/fork/upstream-findings.md`: defects S1–S5 and E1–E18.
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
  commit. The latest is `celld-uf-21688dc`.
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
  - The five staged archives under
    `~/vibestudio-pi-worktrees/artifacts/1.2.0-vibestudio.4-dolt.5`
    match `release.json` for SHA-256, npm SHA-512 integrity, package contents
    and dependency declarations. All 2,108 captured source files match.
  - The host pins `pi-telemetry`, `pi-chord`, `pi-codemode`, `pi-ai` and
    `pi-durable` with `file:` overrides in `package.json`. After publication,
    remove those five entries, regenerate `pnpm-lock.yaml` from the registry
    and verify a frozen install. The declared direct dependencies already
    use the staged version.
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
- **Last full e2e** (release build of `b5742d1`): 47 passed, 1 known
  failure (E2, the R2 etag), 2 skipped (`pi` and `opencode` need live
  model endpoints), and zero unexpected failures. The run used one worker
  and took 560 seconds. See `p5-fork-e2e-full.log`. The local pinned binary
  is `celld-uf-b5742d1`, SHA-256
  `9c1eed52e3f6e21ab084a3d8864bbd7c0ebc42e2d494bd91d2a808af83e56d8d`.
  The added historical row test failed at the expected saved read on
  `60f0bf8` and passed on `f9b25fb` and `b5742d1`.

**Open in the fork, in priority order:**

The pinned CI Linux binary from `0410398` repeated the 51-scenario suite,
including the new two-deployment Dolt transfer. In the first scope 47
passed, R2 recorded its existing ETag XFAIL, Pi and OpenCode skipped
their model calls, and the container load balancer hit capacity while
the runtime reported memory shedding. The container scenario passed
on the same binary in a separate 6 GiB scope with swap disabled.
See `p5-pinned-fork-e2e.log` and `p5-pinned-fork-container.log`.

The next full sweep used the pinned Linux x64 binary from `c8dfc2e`
in the 6 GiB scope. Container load balancing passed. The queue restore
case timed out in the full run. Both durability steps now verify the
enqueue response explicitly. An isolated run on the same binary
passed the crash, restore, and delivery counts. The full sweep recorded
47 passes, the existing R2 ETag XFAIL, and two model-dependent skips.
See `p5-verified-fork-e2e.log` and `p5-verified-fork-queue-repeat.log`.

The full serial rerun on the pinned `c8dfc2e` Linux binary passed 48
scenarios, including queue restore and container allocation. There
were zero unexpected failures, the existing R2 ETag XFAIL, and two
model-dependent skips. It took 605 seconds and peaked at 397 MiB with
no memory-limit or OOM events. See `p5-final-fork-e2e.log` and
`p5-final-fork-e2e-memory.log`. The Windows and artifact workflows
also passed at `e25fcad`.

1. **`python` e2e complete.** Runtime `b5742d1` validates Pyodide's
   typed import-and-re-export adapter before compiling it. The Python
   bundle uses the helper within the existing global code-generation policy.
   The sandbox scenario tested the exact adapter and rejected an arbitrary
   module and a module with an extra section. The Python scenario tested
   ordinary requests, a vendored package, and KV data through restart
   and recovery. See `p5-python-adapter-check.log` and
   [Codegen confinement](codegen.md).
2. **Release artifacts for linux-arm64, darwin-arm64 and win32-x64.**
   The fork artifact workflow builds all four supported targets and
   includes an exact commit and digest in each artifact. Use the
   Windows workflow for platform smoke results.
   - Host `27001d86f` stages CI-provided payloads for Linux, Windows and
     macOS desktop packaging and for the headless server package.
     Staging verifies the pinned fork commit, target, format and digest.
     All 11 artifact and package helper tests passed.
   - [Artifact run 44460685656](https://github.com/panticonic/celld/actions/runs/44460685656)
     built all four platforms at `c8dfc2e`. Host `3a49d5972` pins their
     verified checksums. The four payloads were staged and the Linux x64
     payload reported version 0.6.3.
   - Fork `172f9e9` uses native filesystem paths for locally captured
     LTX files. The Windows suite passed the verbatim path test,
     hello and counter startup, and counter recovery after a forced
     supervisor stop. See [Windows run 44460685503](https://github.com/panticonic/celld/actions/runs/44460685503).
   - Host `aef332742` moved the retired macOS 14 CI jobs to macOS 15.
     The installer still declares macOS 14 as its minimum version.
     The workflow suite passed all 29 checks. See the
     [GitHub runner retirement announcement](https://github.com/actions/runner-images/issues/13518).
   - Host `e54a8f1be` restores the packaged preparation recipe receipt
     when a worktree rebuild reuses the same verified host generation.
     The production build and 24 packaging and generation tests passed.
     The staged headless install resolved the unpublished Pi packages
     through an isolated local registry and installed all 38 vendored
     runtime packages. The published System template still imports the
     removed `@vibestudio/service-schemas/evalEngine` API. The incompatible
     preparation was stopped and joined after the same error recurred.
     The owner must publish the matched template revisions and update
     the release pins before an installed headless candidate can pass.
     See `p5-headless-with-pi.log`.
   - See `HANDOFF-celld-runtime.md`, "What remains".
3. **The P2P carrier for Dolt remotes across deployments is proved.**
   The two-node `dolt-remote-p2p` scenario passed on `b5742d1`.
   It validates native fetch and push, exact commits, a wrong-peer
   refusal and revocation. The P2P stream regression passed alongside
   it. The later P6 work connects the carrier to Chronolog membership and the
   Vibestudio workspace history cells. See the current evidence in §6.
4. **Known limits, documented and unfixed:**
   - A facet RPC in flight is not cancelled on abort; it rejects in
     JavaScript.
   - An unrelated event's failure while it drives an orphan block resets the
     object.
   - From §3b.17: a timer that re-arms a spin is ended once per limit,
     forever; a cut-off event's rollback also rolls back a concurrent event's
     transaction on the same object; and microtasks that V8 drops can belong
     to other objects.
   - E14 now has a closing-checkpoint scenario. A reply property queues a
     transaction from an already-ended event when the runtime reads it.
     The new scenario, the two continuation scenarios, and the facet
     limit recovery scenario pass. The pre-fix binary fails the new
     scenario with the original input-gate refusal. See
     `e14-closing-checkpoint-compat.log` and
     `e14-closing-checkpoint-control.log`.
5. **Upstream defects still open:**
   - E2: the dev R2 etag is a counter.
   - E4: static-assets answers `/index.html` with a 307 and serves a 404
     with a JSON content type.
   - S4: a drain exceeds its release ceiling.
   - S5: a cell is released after a failed restore without proof of
     durability.
   - All four are in `upstream-findings.md`.
6. **Remaining performance evidence:**
   - Host review and integration timings at GAD size. The native checkout
     and disjoint-merge measurements are recorded above.
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
| P5 GAD decentralized        | Fresh default startup and two signed participants passed on managed source `#62`, including private work, reviewed publication, and normal shutdown. The current native installed release check and removal of the remaining legacy storage mechanics are open. |
| P6 Chronolog capability     | Installed group writes passed. The native workspace directory, host ingress, and real GAD cells passed a signed two-node publication with verified file bytes. The two-installed-workspace release test remains. |

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
- **S6a and S6b: done.** The executor fact is dynamic-code, and an agent
  hosts its persistent eval kernel through the general `hostCode` mechanism.
  Host `11be2a5a0` and `ddb4b356d`, Base `4132392` on both feature branches.
  Requests and replies distinguish endpoints sharing one principal id. The
  actual self-call, authority, continuity, crash replay, finite cancellation
  and forked-subagent cases pass. Finite scopes and owner-session targets
  remain session entities by design. See the Nov 17 completion record above.
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
  - The 65-turn product soak exercises collection at the default threshold.
    The real `WorkspaceStateDO` test verifies the same inherited schedule,
    restart, and preservation of state and history.
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
- **Still being measured:** fork latency at 10,000 turns. Activation-loss
  replay already verified restart of an agent cell during a run.

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

### 4.4 Earlier Chronolog port (`feature/celld-host`)

The current port is recorded in §5, item 3. Local port `63194c9` includes
Chronolog main `d1cc92b` and uses the celld adapter in `peer-session`.
Current main has removed the old daemon and SSB transport. The evidence
below belongs to the earlier host architecture.

- Chronolog `f452310` removed its temporary peer-authorization
  debugging route and documented the actual object-owned stream
  lifecycle. Full lint and typecheck, native conformance in UTC and
  Pacific/Apia, and the two-node test including replay and restart
  passed against the packaged Linux binary from `0410398`. See
  `chronolog-pinned-release-check.log`.

- Chronolog `12554c8` starts the celld materializer's replay at the
  exact first-parent commit preceding a late transaction. The native
  conformance test asserts that it skips the unchanged prefix. The two
  time zones produced the same replay digest, and both replicas agreed
  after late insertion and restart on the pinned `c8dfc2e` Linux binary.
  Typecheck, lint and the focused materializer tests passed. See
  `chronolog-exact-replay-check.log`.

- Chronolog `f0a5259` moved the signing seed into the native node
  class secret. The pinned runtime refused to start without the seed
  and passed the two-node test through replay and restart when the
  secret was installed. Chronolog `b9f401a` moved feed storage and
  signing out of the old SSB transport. Typecheck, lint, 99 focused
  tests and the two-node native test passed on `c8dfc2e`. See
  `chronolog-class-secret-check.log` and
  `chronolog-feeds-cutover-check.log`.

- Chronolog `5a826eb` moved governance and health RPC adapters out of
  the old daemon. The 87 focused tests, typecheck, and lint passed. Two
  native nodes converged on `c8dfc2e`. The restarted node served health
  and Prometheus metrics for the converged revision. See
  `chronolog-native-rpc-check2.log`.

- Chronolog `702cc02` accepts the legacy static membership document
  from the node class secret and pins it to the native execution
  manifest. Both governed and static two-node runs passed on
  `c8dfc2e`, including writes on both nodes, late replay, and restart.
  The static node refused dynamic genesis. Typecheck, lint, and 86
  focused tests passed. See `chronolog-static-group-check3.log`.

- Chronolog `a9b1aa2` prepares fork seed plans from an exact native
  revision and commit. Independent UTC and Pacific/Apia runs on
  `c8dfc2e` produced identical plans, rejected a stale revision, and
  recreated the source's application tables in a new Dolt object.
  Both objects rejected reapplying each batch and kept the same table
  digests. The 108 focused tests, typecheck, and lint passed. Native signed
  archive operations and the group CLI still need adapters. See
  `chronolog-native-fork-seed4.log`.

- Chronolog `eb3f1a3` uses the shared custody providers to provision the
  native class secret by reference. The `c8dfc2e` CLI sealed an exact
  synthetic provider value against a loopback S3 fixture. Failed reads
  stopped provisioning and remote errors were redacted. The 51 focused
  tests, full typecheck, and lint passed. The command provider ran on
  Linux. Other provider checks use fixtures. See
  `chronolog-secret-provision-check2.log`.

- Chronolog `8b69e1f` captures the exact native revision and
  transaction-log digest in signed snapshot evidence, using the same
  evidence format as the legacy adapter. The UTC and Pacific/Apia
  native fixtures produced the same signed manifest and used its
  digest to seed a fresh object. The 20 focused tests, typecheck, and
  lint passed. Native archive transport and restore are still open. See
  `chronolog-native-snapshot-evidence.log`.

- Chronolog `aa64e21` runs the shared conformance replay fixture in
  independent celld nodes under UTC and Pacific/Apia. Both reached
  revision 2 at commit `148871e88e2489f8992004bc75a328a6c9240929` with
  identical replay digest `1230bfaab1a6e0dbd3eb5a152a4bd54483c3a34da45522e6cd4df43da5faf77b`.
  It also removes the old event anchor now that the object owns its
  peer streams. The real two-node group test passed, including replay
  and a peer restart, alongside 40 conformance and transport tests,
  typecheck, and scoped lint. Both native tests used `b5742d1`.
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
  1. The native conformance replay and two-time-zone comparison are verified
     above.
  2. The transport now uses object-owned streams and waits only for inbound
     admission. The fork handles the version 0 Ed25519 key encoding.
  3. Cutover deletions:
     - delete `apps/chronologd`, `materializer-doltlite`,
       `@dolthub/doltlite` and its pnpm patch, and the SSB network layer of
       `transport-ssb`;
     - port signed snapshot archives and wire fork seeds to the native
       group CLI. The feed package, native static membership, fork-seed
       planner, and reference-based secret provisioning landed above.
  4. node-core tests and a chaos harness against the celld host.

---

## 5. The plan to finish the whole project (single-track, in order)

Each step ends with its gate green, the per-stream handoff updated, and a
commit. Vibestudio, the templates, Pi and Chronolog stay local; the owner
pushes them.

1. **Close P4.** Read `HANDOFF-dolt-redesign.md`, "P4 continuation (2026-11-17)", first.
   1. The stopped-sweep reruns and unfinished lane selections are complete.
      Outcomes and repaired defects are recorded above.
   2. The observed saturation was host causal-read fanout and is repaired
      above. The fresh strict steering completion case passes with resolved
      counts. Keep the separate retained-collaborator follow-up result distinct.
   3. **S6b complete:** both held branches are merged locally; the real
      self-call and targeted lifecycle/fork regression checks pass.
   4. The 65-turn model-free product soak and the real
      `WorkspaceStateDO` compaction check pass.
   5. The agent-cell restart probe passes through activation-loss replay.
      Measure fork latency at 10,000 turns.
   - **Gate:** §10, row P4, of the redesign.
2. **P5, decentralized GAD** (about 20 agent-days, the largest phase). The
   design is `docs/dolt-redesign.md` §5.
   - Start fresh workspaces in a canonical Dolt database and put each
     participant in their own work cell. Use Dolt branches for contexts
     (§5.1). Keep ownership, publication approval and semantic conflict
     choices in Vibestudio.
   - Integration by F1 merge, `pull` and fast-forward `push` (§5.5).
   - Grants, and approval as grant issuance (§5.6).
   - Verify normal creation and retirement of work cells (§5.7). Existing
     recovery evidence is retained. Further deliberate process-loss and
     lost-reply checks are outside the release gate.
   - A derived `commit_index` and `content_roots`.
   - Delete the fact-map, radix and frontier machinery, and GAD's private
     hash chain left from S1.
   - **Gate:**
     - the provenance plan's acceptance matrix, re-expressed over commit
       hashes;
     - §5.13, the decentralization gates;
     - a fresh current-release installation, private save, normal restart,
       independent reader, reviewed publication and integration;
     - the size gate using a representative fresh workspace;
     - checkout p95 within 20 ms with `fsync` and 10k local rows (the fork
       measured 14–16 ms).
   - Do it in vertical slices, like P4, and never leave two models in force.
3. **P6, the Chronolog capability.** Design: `docs/dolt-redesign.md` §8.
   The owner selected the current Chronolog main as the release target.
   Its `d014879` revision uses the owned peer-session architecture.
   It replaced the daemon, RPC, node-core and SSB packages that the earlier
   native port used. Keep the older worktree as verification evidence.
   Port through the current device, storage, execution and peer transport
   interfaces in `/home/werg/chronolog-worktrees/celld-peer-session` on
   `feature/celld-peer-session`.
   The current port has a celld SQLite record store and peer-session entry.
   Package `41f878a` retained a signed event, device identity and exact
   retry across process restart on the pinned runtime. Commit `31600b7`
   uses current peer-session framing over object-owned Iroh streams. Two
   native peers wrote to the same authenticated history, then one restarted,
   retried its exact operation, and followed a new write. The 67 focused
   tests and typecheck passed. The native SQL owner policy and Vibestudio
   capability still need their current adapters. See
   `chronolog-current-celld-package-check.log` and
   `chronolog-current-celld-iroh-check4.log`.
   Commit `4872da9` connects the current participation transport to admitted
   Iroh endpoints, including incoming handoffs and late completion after
   cancellation. The 39 focused tests, type check and targeted lint passed.
   Two nodes ran the independently installed package
   `chronolog-peer-session-4872da9f.tgz` against the pinned runtime. Both
   wrote, then the restarted peer retained its identity, retried exactly and
   received the next write. See `chronolog-celld-admitted-transport-native.log`.
   Commit `870fca2` persists the current signed control registry in a celld
   object. It refuses overlapping owners of the same application and joins
   pending control operations on close. The package independently installed
   on two `c8dfc2e` nodes retained the exact selected generation after a
   process restart and retry. The 20 focused tests, type check and targeted
   lint passed. See `chronolog-celld-control-native.log`.
   Commit `bd2def2` imports a host-managed Ed25519 signer from a celld
   class secret and persists only its public identity and storage binding.
   Commit `36495bc` documents enrollment. All 43 focused tests, type check
   and targeted lint passed. An independent installation of
   `chronolog-peer-session-bd2def2d.tgz` on the `a6dce5d` CI runtime
   verified the same signer and storage identity after restart, the
   class-only binding, and the absence of plaintext signing bytes in the
   runtime store. See `chronolog-celld-current-signer-native2.log`.
   The same installed package passed an invitation exercise on two native
   Iroh nodes. The fixture verified the second device had no reader grant
   before the owner signed a new control generation. The invitation carried
   no epoch key. The invited device received it and the verified existing
   prefix over Iroh, wrote on the shared history, and retained the exact
   enrollment, generation and retry across restart. See
   `chronolog-celld-current-invite-native3.log`. This exercised current
   Chronolog enrollment with a host-managed signer in a fixture. The
   Vibestudio invitation endpoint and its approval flow remain open.
   Chronolog `8282562a` adds signed invitation envelopes to the current
   celld package. The invited device validates its member key, the active
   administrator, the selected generation and the signed Iroh address, then
   persists the authority before opening the session. An independent
   installation of `chronolog-peer-session-8282562a.tgz` refused an altered
   route and accepted the exact envelope on two native nodes. The invited
   device acquired the epoch key over Iroh, and both devices retained their
   writes and authority after restart. The 47 focused tests, type check and
   targeted lint passed. See `chronolog-current-celld-enrollment-native.log`
   and `chronolog-current-celld-enrollment-check.log`.
   Fork `2ec8d17` adds the principal row and transaction identities used by
   current Chronolog SQL. The executor pins both to one program and the engine
   digest identifies the changed execution profile. All 75 focused Rust
   tests passed. A locally built runtime verified exact integers above the
   JavaScript safe-integer bound, identity cleanup after rejection, validation and restart
   on a real Dolt cell. See `celld-current-chronolog-identity-native-exact.log`.
   Vibestudio `338d886d3` locally pins all four verified `82a5781` artifacts.
   The clean current Chronolog SQL package `870fca2e-sql` passed 71 focused
   tests for owner rows, protected schema, deterministic replay and final
   prefixes. A separate Node consumer installed the archive and passed its
   SQL verification. This used Chronolog's patched DoltLite 0.11.29 addon.
   The celld engine uses DoltLite 0.51.11 with its own execution digest.
   The workspace materializer still needs to bind one verified profile
   end to end. See `chronolog-current-sql-baseline.log`.
   The fork's `celld-spec10-v3` profile implements the current
   `chronolog_row_key()` and `chronolog_value_equal()` functions and
   restricts policy-column updates to registered lifecycle triggers.
   All 77 Dolt executor tests passed, including vectors taken directly
   from the current patched addon. A real Dolt cell installed the
   exact generated current Chronolog schema and triggers. It accepted
   owner writes and a column grant, rejected a foreign-row write at
   the correct step and rolled back that program. It also refused a
   case-only change outside the grant and a direct metadata write.
   An owner changing the row key atomically revoked the grant, and
   the results survived restart. See
   `chronolog-current-generated-owner-native.log` and
   `celld-current-owner-lifecycle-green.log`.
   Fork `82a5781` passed all 48 model-free runtime scenarios, the
   documented R2 expected failure and two provider-dependent skips.
   That run peaked at 569 MiB with no OOM or limit events.
   The Linux, macOS and Windows artifact matrix is run 44518812004.
   The four downloaded binaries matched their CI receipts and pinned hashes.
   This fixture prepares the schema and grant as trusted setup. The
   current materializer still needs the native transaction adapter for
   signed schema and platform operations.
   The `celld-spec10-v4` profile also protects virtual tables and the
   physical vector chunk tables which carry no row owner. A registered
   application table's triggers may maintain its indexes. Direct
   application writes to those indexes remain outside that authority.
   All 80 executor tests passed. A native cell installed the current
   generated owner rows and an application index, with the `sqlite_vec`
   compatibility flag. It refused direct index and vector-chunk writes,
   applied an owner-checked index update, rolled back the earlier step of
   a refused transaction and produced the same rows after restart.
   See `celld-current-owner-protected-index-acceptance.log` and
   `celld-current-virtual-owner-test2.log`.
   The `celld-spec10-v5` runtime lets the trusted materializer interleave
   its platform writes with consensus SQL in one native transaction.
   A savepoint can roll back a refused program while the encompassing
   transaction retains its rejection log. If a write exceeds the engine
   budget and SQLite aborts the encompassing transaction, both layers
   close and no earlier writes survive. The real worker probe verified
   exact identity, matching random sequences, foreign keys, protected
   writes, rejection logging and restart. The runtime reports whether the
   actual cell enables sqlite-vec alongside the engine digest. See
   `celld-chronolog-program-tx-native.log` and
   `celld-chronolog-program-tx-executor.log`.
   The `celld-spec10-v6` runtime adds detached reads by exact commit hash.
   An independent read-only connection resolves the saved hash and uses the
   cell's active vector module. Current Chronolog `89d3dc87` binds these
   readers and the native transaction to its existing signed-schema
   materializer and exposes them through the optional `/sql-celld` package
   entry. The clean `chronolog-peer-session-89d3dc87-celld.tgz` installation
   opened a signed application, accepted two writes, durably logged a
   refused command and a write interrupted by the step budget, and kept
   an older reader pinned while the live history advanced. A restart
   recovered the same device, generation, successful events and rows.
   The runtime suite passed 48 scenarios, with the known R2 failure
   and two provider-dependent skips. It peaked at 592 MiB with no
   memory-limit or OOM events. See
   `chronolog-current-celld-sql-package.log`,
   `chronolog-current-celld-sql-probe3.log` and
   `celld-chronolog-snapshot-full.log`.
   Chronolog `354d148d` keeps device and peer records in the Dolt cell's
   private runtime KV. The clean package `chronolog-peer-session-354d148d-celld.tgz`
   joined two native nodes through a signed invitation over Iroh. Both
   wrote SQL rows as distinct owners, saw the same data and event history,
   and retained the same identity, authority and rows after restart.
   Both peers refused a query of the runtime KV tables. See
   `chronolog-current-celld-iroh-sql-native.log` and
   `chronolog-current-celld-private-package.log`.
   A second clean-consumer run on the same package rejected a signed write
   into another participant's row. Both peers kept the exact prior rows,
   including rolling back the denied writer's earlier insert from the same
   transaction. See `chronolog-current-celld-iroh-owner.log`.
   A signed generation change then restored the same earlier history on
   both nodes. Both removed the excluded SQL row and event, preserved the
   surviving data and public signing keys, and reopened the selected
   history after restart. The reader already open before the change still
   returned its original view. The fixture delivered the signed generation
   change to the second node explicitly and synchronized data over Iroh.
   See `chronolog-current-celld-iroh-replay3.log`.
   Chronolog `758fa47` signs and verifies a protected workspace publication
   against the publisher role, selected group generation, destination node
   and workspace, and both exact heads. The tests refused new destinations,
   changed heads, signature tampering and a proof after publishing authority
   was revoked. The host carrier still needs to consume this proof.
   Chronolog `af670ca` allows a workspace node with an explicit relay grant
   to carry the account's independently signed attempt. The coordinator
   verifies the account's current write grant and signature. Local retries
   are scoped by account and operation ID. Vibestudio `9d7818f88` keeps one
   encrypted signing key per verified hub account. The process-authenticated
   workspace-child port checks current workspace membership before and after
   loading the key. The 80 focused host tests passed.
   Chronolog `bd15feb` adds member-scoped reads and live queries, receipts by
   exact transaction hash, and the precise failing program step on rejection.
   The two-node packaged run confirmed account-signed row ownership through
   the follower, duplicate operation IDs under different keys, exact retry
   across restart, member-only query, live query cancellation, stale revision
   refusal, and atomic rejection at the offending SQL statement. It retained
   the earlier independent SQL, replay and Iroh authorization checks.
   See `chronolog-current-celld-group-data-iroh.log`.
   Chronolog `a1f9813a` persists group genesis, the signed invitation, and
   admitted peer endpoints. An offline-installed archive used two separately
   keyed celld nodes and two account keys. It verified creation retry,
   administrator-only invitations, denial for an unrelated joiner, a second
   account admitted through the same follower, account-owned rows, and an
   unchanged invitation and transaction after retry. See
   `chronolog-current-celld-group-control-iroh.log`.
   Vibestudio `f84d16bf8` defines the reviewed transaction and member read
   surface. The dispatcher test refuses a changed request until another review
   and releases the signing key only after approval. Read calls request the
   public identity. Host typecheck, targeted lint, and 82 focused tests passed.
   Host `efeff4fcf` wires this service to its workspace's deployed celld
   node. The host build accepts a verified, clean local Chronolog package
   directory via `VIBESTUDIO_CHRONOLOG_PACKAGE_ROOT`, fingerprints the
   exact package, and packages both trusted classes in its existing runtime
   executable. The scoped SQLite directory routes `chronolog/1` to an
   independent Dolt cell for each installed group. Class credentials stay
   out of script variables. The native route shares one durable Iroh
   identity per workspace and one bounded WebSocket handoff within a node.
   Chronolog `3ca2381` passed two independent workspaces with signed account
   writes in two groups, administrator-issued invitations, exact receipts
   and reconnection after restart. The combined Vibestudio runtime passed
   those account and restart checks through the real private workerd
   router and refused a request missing the host dispatch secret. This
   used the exact v6 celld binary. See
   `chronolog-current-celld-workspace-node6-iroh.log` and
   `chronolog-combined-host2-iroh.log`. Host typecheck, targeted lint,
   109 unit tests and eight runtime artifact tests passed.
   Fresh managed source `#63` started normally and installed native
   workspace history. Its agent code could not see the Chronolog namespace.
   Host `dc5dcdae4` and Base `f5a3dae` expose the same typed Chronolog
   operations to panels, workers and eval through the portable runtime.
   All four Base type configurations and 56 runtime/help tests passed.
   Fresh managed source `#64` reached the approval queue and exposed a
   missing review label for Chronolog. Host `20baa2e45` supplies the
   reviewed capability descriptions and binds each approval to its exact
   request. Fresh managed source `#65` then created a group, held a write
   in the real approval queue, resumed it after a one-time approval, and
   verified that the signed-in account owned the row. This used Base
   `f5a3dae`, the sealed Chronolog `2e85c17` archive, and celld
   `b78e4d0`. See `chronolog-managed65-1119-evidence.log`. No model
   calls were made. The fixture and registry shut down. The managed
   instance peaked at 4.02 GiB with zero OOM events and hard-limit hits.
   The earlier `#63` managed instance peaked at 3.97 GiB with the same
   zero counters; its fixture and registry also shut down.
   Chronolog `2e85c17` checks publication signatures through the selected
   native group session and the receiver's own workspace node identity.
   Two real celld nodes then accepted a signed exact Dolt `main` transition
   over Iroh in a separate fixture. A tampered signature left the shared
   head unchanged. The accepted update carried the exact contents and head.
   The recording is `chronolog-current-protected-dolt-iroh.log`. This
   used a fixture graph; the installed workspace GAD carrier still needs
   to use the same verification and enforce transfer of its referenced
   content blobs.
   A fresh two-node native run with the sealed Chronolog `93b8dfb`
   archive replayed two groups, rejected a proof naming the wrong Iroh
   sender, and left the head unchanged after a forged signature. After a
   restart with the coordinator offline, the follower served retained
   reads, saved a signed write, and reconciled the same attempt when the
   coordinator returned. The original invitation retried byte-for-byte
   after its endpoint hint changed. See
   `chronolog-current-offline-carrier4-iroh.log`.
   Host `74bbc69ce`, Base `ae0fcd3`, and Chronolog `2e969ab` add
   reviewed history sharing and publication. The receiver first gives the
   signed sender a native grant to one private staging branch, checks every
   referenced digest, and confirms current Chronolog authority again before
   applying the exact protected-main transition. Host and portable type
   checks passed for the changes they compiled. On the preceding combined
   Base candidate, four type configurations and 223 focused tests passed.
   The signed two-node fixture used the actual Chronolog workspace directory,
   host ingress handler, and Base GAD history cells. It rejected a publish
   with a missing file and rejected unrelated and incorrect bytes. After the
   exact file arrived, it advanced shared history to the signed head and
   returned the same result on retry. See `native-gad-chronolog6.log`; this
   fixture peaked at 369.5 MiB and shut down both nodes. The two histories
   were linked by an exact test seed from the receiver. A normal signed
   publication requires a shared ancestor. The operator workflow below
   prepares that ancestor for a fresh installation.
   Host source preparation with the sealed `2e969ab` archive passed, along
   with 61 focused host tests. Managed source `#66` passed the new
   sharing command and current-head query through the installed approval
   surface. The signed-in administrator approved sharing, the workspace
   returned the exact history revision and its live Iroh address, and
   the fixture retired. This run peaked at 3.99 GiB with zero OOM events
   or hard-limit hits and made no model calls. See
   `chronolog-managed66-1119-evidence.log`.
   Host `caf30295e` and Base `d88407b` recover an interrupted host
   projection on retry and preserve the exact signed receipt after
   shared history advances again. The native two-node test returned
   the original receipt without moving the newer head. See
   `native-gad-chronolog-receipt.log`. Targeted lint and all 15 host
   ingress and protected history tests passed in
   `chronolog-history-receipt-host.log`. Host `b028589f0` adds a regression
   that interrupts the host projection after the native save and retries
   through the ingress. All ten protected publication tests passed.
   Current Chronolog `b97c42b` also requires both workspaces to enable
   sharing with the same group before exchanging published history. A
   locally approved workspace administrator may enable it with a group
   administrator or publisher grant. The new test refuses signing and
   sending for an unselected group. The two native nodes refused the
   signature before sharing was enabled, then accepted the same transition
   after the publisher enabled it. See `native-gad-shared-egress.log`.
   The sealed `b97c42b6` celld package SHA-256 is
   `f45bb42051464d44df7335c024b1fadb58f9d3013afa51b63cbcfc2638fa6261`.
   Chronolog `4d32d98` adds a Linux release job step for the celld package
   and its build attestation. Its local package SHA-256 is
   `680d7dfb4c6fb9e92f1340973e74efab5c7196e0d05ed6bfe5719e3803d4617b`.
   Host `3476a4210` verifies the reviewed provenance fingerprint when
   `VIBESTUDIO_CHRONOLOG_PACKAGE_MANIFEST_SHA256` is supplied with the
   staged package root. The local package fingerprint is
   `f51a0820bf14af8b83136830e75c946fc90777d276296d3e80e6faa382536876`.
   The exact archive passed the host loader, and all eleven focused host
   build-input tests passed. The same archive passed the two-node
   publication test in `native-gad-release-candidate.log`. The Chronolog
   release workflow is local.
   A separate native copy probe fetched only one branch at the selected
   published head. The new workspace kept its original node identity.
   The probe refused a private branch, the wrong peer, an incorrect
   event, and any attempt to replace an installed history. See
   `native-gad-copy-seed.log`. Base `cf6a457` and Host `7ad54a8c1`
   add a local operator workflow. An existing workspace exports its exact
   published branch and reachable file contents. A newly registered workspace
   verifies the bundle and publishes its initial history through normal
   protected creation before activation. The recipient keeps its own node
   identity. The operator setup is documented in the host's
   `STATE_DIRECTORY.md`. The real native bundle test verified the exact
   revision, file bytes, distinct identities, grant retirement, and retry.
   See `native-gad-seed-bundle.log`. All 21 focused host tests passed.
   `native-gad-seed-protected.log` also composed the production protected
   host ref publication with the native copy, observed the creation gate,
   and reopened the recorded origin after removing the staged bundle.
   Chronolog main advanced to `d1cc92b`. The local port merged it at
   `63194c9`, keeping the native materializer's rollback recovery and using
   the upstream owner's verified authority API. Type checking, targeted
   lint, and all 125 focused tests passed. The new sealed archive SHA-256 is
   `7f0135185732929fa572ed97dbeb0cd450589d7434e6c1bea72ba32e0a75c21c`.
   The host loader verified its provenance fingerprint
   `62811e8fb35b7befb86262426ea5718089e52caaa1059a5221b55e0bfdaa03a6`.
   With that archive, two nodes replayed signed SQL, preserved distinct row
   owners, refused a cross-owner update, and recovered the selected
   generation and read views after restart. The workspace directory also
   reconnected two groups, and the signed GAD publication retained its exact
   receipt. See `chronolog-current-main-d1cc92b-check.log`,
   `chronolog-d1cc92b-replay-native.log`,
   `chronolog-d1cc92b-workspace-native.log`, and `native-gad-d1cc92b.log`.
   Chronolog main advanced to `88716db` with reviewed control prefixes
   and retained publication outcomes. The local port merged it at
   `f38b9d5`. Full lint and type checking and all 83 focused tests
   passed. The host loader verified the clean celld package archive
   SHA-256 `4a4f8887ff71eb53ac9af37453de32de2e1cafb5a68e20d7e9f5678061c8323f`
   and manifest fingerprint
   `c2a71f8d45efb2820231db8dd26aed137af0c15d689c7d7df84505fb202d79e0`.
   See `chronolog-current-main-88716db-check.log`,
   `chronolog-88716db-package.log`, and `chronolog-88716db-host-package.json`.
   The exact archive also installed on two isolated Ubuntu 22.04 celld
   nodes. Separate signed participants joined two groups, wrote on both
   nodes, read the exact owner rows, and reopened both groups normally.
   The installed Vibestudio pair uses the SQL programs encoded by this
   same package. See `chronolog-88716db-workspace-native.log`.
   The two-node replay fixture also passed using that exact archive and
   runtime. Both nodes discarded the excluded SQL tail after the signed
   control change, agreed on the retained rows, and preserved distinct
   row owners after a normal restart. The older read view stayed pinned.
   See `chronolog-88716db-replay-native.log`.
   Chronolog main advanced to `1e4e840` with push discovery and
   ordinary replication independent of hosted discovery. The local port
   merged that revision at `f7351ce`. Lint, type checking, and all 94
   focused peer-session tests passed. The host verified the clean archive
   SHA-256 `d626497023d3a2c1a78b8b6d5066ba582faf6afe74b8cacf23297284a7debd75`
   and manifest fingerprint
   `8a93d55d66ddfa419d09a847ee8fc38a27b0f68e622f0cb2a041e4aae224d35b`.
   The exact archive passed both native two-node checks on runtime
   `61862f2`. Signed participants wrote and reopened two workspace groups,
   and SQL replay preserved both owners, refused cross-owner writes, and
   converged after the signed generation change. See
   `p6-chronolog-1e4e840-check.log`, `chronolog-1e4e840-host-package.json`,
   `chronolog-1e4e840-workspace-native.log`, and
   `chronolog-1e4e840-replay-native.log`.
   Host `f64f4d2b2` stages the Chronolog package in application release
   builds. Once the owner publishes the Chronolog release run, set
   `CHRONOLOG_RELEASE_RUN`, `CHRONOLOG_RELEASE_SOURCE`, and
   `CHRONOLOG_RELEASE_SHA256` to its run, source, and reviewed archive digest.
   The host checks the successful release workflow, exact archive and
   package provenance before compiling. The 29 workflow checks passed.
   A local fixture exercised the production staging script with the sealed
   archive and a fixture GitHub response. See
   `chronolog-release-staging-host.log` and
   `chronolog-release-staging-ci-probe.log`.
   The installed pair check runs after the matching Host `57f501437`
   and Base `05ecc1b` build. It approves creation
   of the seeded workspace, verifies distinct accounts and nodes, and
   compares the exact published file bytes and retry response on the two
   workspaces. On the combined host, all 15 focused startup, history
   migration and retained-host selection tests passed. The live two-host
   and version handoff results are pending.
   System-testing `5296454` removes a duplicate mission-edit permission
   in the merged runner manifest. The release parser rejects the earlier
   manifest and accepts the corrected one, and the combined release built.
   Its first startup test found that the hub opened distinct newly
   registered workspaces when it restarted. Host `9ff82bded` leaves these
   registrations unopened until first route and provides the native error
   reason on export. All 83 focused hub and startup tests passed.
   The next installed attempt reported SQLite's “string or blob too big”
   for the ordinary workspace's complete history pack. E19 scopes the
   SQLite length override to native pack operations. The earlier release
   reproduces the failure in `dolt-remotes-large-pack`. Fork `4cb52cb`
   passed all 14 focused native Rust tests, the large-pack end-to-end
   scenario, and the existing remotes durability scenario. The installed
   rerun exported a 3,712,207-byte pack and 6,278 files from its source
   workspace with the rebuilt Linux candidate. First open of the
   destination then exposed a circular wait: seed installation called
   the public mutation entry that waits for semantic readiness. Host
   `80f977d4c` takes the same publication lease directly during bootstrap.
   All 20 focused publication and startup tests passed, including seed
   installation while readiness is pending. The destination then
   completed that publication on the installed app. Its launch record
   binds the exact seed approval, and the staged pack matches its digest.
   Creation review initially failed because a declared extension was
   read from the destination's bootstrap template. Host `9c2fdab81` and
   `fb3166562` now give review the imported state's exact source paths
   from first graph discovery and exclude the provisional host-build
   inventory from imported unit admission. The 20 focused host checks
   include reopening the seed and reading the exact manifest from the
   discovered path. The installed restart ran under
   `vs-verify-imported-graph-reopen.service`.
   The installed restart preserved the seed and recorded 43 admissions
   through the creation review. Two signed accounts on separate nodes
   joined the same Chronolog group, wrote from both sides, rejected
   Bob's attempt to modify Alice's row, and read the same owner rows.
   The fixture uses the coordinator's original loopback port because
   its celld discovery and relay are disabled. The workspace administrator
   binds sharing locally, and Bob remains a publisher in the group.
   The protected project-file publication reached authority checking
   before the three-minute eval deadline. The longer run passed review
   and published the saved project-file revision on the destination.
   Host `f3b5cc909` distinguished an HTTP 403 on the native staging-grant
   request and passed 12 focused checks. Host `351560f93` routes the signed
   Chronolog directory request through the loopback history ingress, with
   separate native token and proof verification at the gateway. All 96
   focused egress and ingress tests passed. The installed retry advanced
   the original workspace to the exact signed revision and read back the
   exact project-file bytes. Retrying returned the same receipt. The test
   shut down normally after peaking at 7.29 GiB with zero hard-limit hits
   or OOM events. See `p5-p6-installed-shared-history-evidence.log`.
   The separate epoch-1 test host `aa1993329` compiled with app version
   `1.0.0` and was retained with the production snapshot script. The
   installed epoch-0 host rejected an incompatible minimum app version,
   then reached the publication review once the candidate specified
   `1.0.0`. The review still tried to interpret the new manifest to
   compute client trust. Host `ea781f163` requires fresh critical
   confirmation of the exact source diff and lets the compatible target
   host load the new manifest and trust configuration. All 78 focused
   publication checks passed. The installed fixture approved and committed
   the saved candidate, persisted the epoch-1 launch record, and the hub
   handed off to the retained app. That app rejected the original epoch-0
   scaffold before reaching the protected manifest. Host `16088a5da`
   supplies the protected launch configuration before resolving an
   existing workspace. All 31 focused loader and creation tests passed.
   The epoch-1 test build `9021bd265` reached template bootstrap, where a
   second scaffold check still interpreted the old manifest. Host
   `370b4c4eb` keeps the scaffold receipt and source presence checks while
   using protected configuration for an already published workspace. All
   56 focused bootstrap and loader tests passed. The replacement epoch-1
   build `1cf59049e` reached dependency preparation, which rejected the
   surrounding hub's epoch-0 release resources. Host `cbdb73278` binds
   retained children to their own packaged workspace resources and exact
   template pins. All 82 focused hub and snapshot tests passed and all four
   host type configurations passed on `cbdb73278`. With its own release
   installed, the target reached semantic activation, which reentered
   scaffold preparation. Host `c763ed04d` keeps the protected-source choice
   on the bootstrap instance for the full startup. All 27 focused tests
   passed, including repeated preparation. Its epoch-1 build `5f726f46d`
   has its own prepared release. The test launcher refused the earlier
   hub build before opening the retained child. The retry rebuilt the hub
   release at the exact integration commit. The retained app reached
   workspace reconciliation. The managed test launcher then failed on
   its local model discovery probe before running the history assertion.
   The ordinary workspace server reopened the same previously approved
   workspace on epoch 1. The original signed session read the exact
   published event and manifest bytes, the retained package's server
   entry ran, and shutdown completed. See
   `p5-installed-epoch-normal-ready-evidence.log`.
   All four host type configurations passed on `16088a5da`. The first-time
   migration fixture used ordinary startup on `b5742d1` to save a private
   copy, then restarted the same workspace on E19. The owner reopened the
   exact context and original file bytes, the second participant could not
   access them, and the shared main event stayed the same. Both starts
   stopped normally. See `p5-installed-legacy-transition-evidence.log`.
   The exact E19 Linux candidate independently replayed the saved real
   snapshot and verified 5,135 files across 135 repositories. The native
   export was 4,278,036 bytes and the participant fork took 386 ms in this
   run. See `p5-e19-actual-replay-materialization.log`.
   The installed replay parity check passed for the original published
   event and two private events saved through the legacy service. Exact
   file identity, bytes and full-file blame matched after replay. The
   owner retained the later private working revision, the second copy
   could not read it, and shared history stayed unchanged. Both starts
   stopped normally. See `p5-installed-legacy-parity-evidence.log`.
   Base `79deb2a` saves the reviewed template installation in the same
   native commit as the participant revision. Host `98ef0788f` checks that
   the exact candidate carries that installation before review and passes
   the claim through the approved native publish. The receiver checks the
   saved event again before advancing shared history. The native fixture
   refused an omitted or changed installation, published the exact
   metadata-only event, read back the receipt and replayed the same
   publication. All 46 focused host and seven template workflow tests
   passed, as did the four host and four Base type configurations. Base
   `028d4eb` handles the message payload variants found by the combined type
   check. The paired release built and the installed Linux check saved and
   published through the authenticated shell. It read the new receipt at
   the exact event, confirmed the original receipt was unchanged, and
   returned the same result on retry. Both fixtures stopped normally. See
   `p5-native-template-publication-evidence.log` and
   `p5-installed-template-publication-evidence.log`. The installed check
   exercises save and publication from an exact fixture pin. The template
   extension's remote source workflow was tested separately in its focused
   suite.
   Host `676d51652` and Base `79ec699` seal the old history freeze only
   after validating the installed receipt and attaching participant
   history. A sealed reopen verifies the exact receipt and freeze
   generation without rebuilding the old replay plan. A replaced freeze
   cannot be sealed with an older receipt, and the sealed source cannot
   resume writes. All 26 focused host and 69 Base tests passed. The
   installed test saved a private copy on the older runtime, migrated it,
   and reopened it on a third start using the sealed record. The owner
   read the exact revision and bytes again, and shared history stayed
   unchanged. All three starts stopped normally. The four host and four
   Base type configurations passed, and the exact paired release built.
   See `p5-installed-history-seal-evidence.log`. The legacy source still
   serves retained data and its other runtime duties.
   Host staging head `ff105da3d` captures the sealed source's content
   retention list once and binds it to the verified replay receipt. Later
   starts can run content collection and recover publications without
   asking the frozen source for its roots or outbox. The regression test
   reopens without the legacy port, preserves old and current content, and
   removes an unreferenced blob. The list stays on disk between sweeps,
   and a missing list stops collection before deletion. All 74 focused
   tests passed. The paired release build and installed restart check are
   queued in `vs-verify-staged-legacy-retention.service` after the benchmark
   releases its workspace and 15 GiB of host memory is available. See
   `p5-retired-legacy-retention-disk.log`.
   Staged Host `6342a9acd` and Base `1927e04` preserve the original
   template initialization receipt in canonical participant history. The
   native store binds the receipt to the frozen replay, checks that the
   initialization event belongs to that history, and refuses replacement.
   On restart the host verifies the exact original template pin using the
   canonical receipt. It reads the old source once to copy the receipt, or
   when paired with an older Base. All 75 focused Host tests and the
   projected Base receipt test passed. The installed check is queued in
   `vs-verify-staged-root-initialization.service` after the retention
   check. Evidence is in `p5-native-root-initialization-host-final.log` and
   `p5-native-root-initialization-base-test3.log`.
   Host `f8e79c1e8` and Base `4d12ca3` also move the verified freeze seal
   into canonical history. The host records it after the legacy source is
   sealed and the retention list is written. Reopening with that seal
   verifies the replay receipt and resumes without the legacy freeze RPC.
   The original read shape and the recovery path for older Base builds
   remain supported. All 77 focused Host tests and two Base tests passed.
   The same queued installed restart now covers both the canonical seal
   and the original template receipt. See
   `p5-native-replay-seal-host-final.log` and `p5-native-replay-seal-base.log`.
   The queued Host head `c2c7e7f1f` also requires a template exchange to
   match the requested working revision before preparing file changes.
   Both directions now reject a stale revision without materializing or
   changing either side. All 15 focused exchange and development service
   tests passed. See `p5-template-exchange-exact-head.log`.
   Host `78a2d0c25` routes template exchange and adopted-repository reads
   through the verified caller's participant copy once native history is
   attached. The plan is bound to the participant, and native import
   verifies the saved revision and keeps scanned files private. Two
   participants got different exact source plans, and a participant could
   neither apply nor replay the other's import. All 99 focused Host tests
   passed. Full Host type checks and paired release preparation are queued
   with this head. The normal installed template exchange itself remains to
   be exercised. See `p5-native-template-exchange-host-final.log`.
   Host `7b96d7178` prepares development builds from a single revision of
   the caller's participant copy across Host, Base, Personal, and System.
   It refuses mixed revisions before creating a run root. All 85 focused
   tests passed, including one projection read for a set of repositories.
   The same queued paired build and full Host type checks use this newer
   head. Installed development build verification remains pending. See
   `p5-participant-development-build-tests.log`.
   Host `5d269d118` reads settings previews from protected main and authors
   settings changes in a temporary native participant copy. The protected
   publication gate still uses the authenticated initiating caller. The
   focused run passed 72 tests, and all 11 writer tests passed again after
   the final source-read adjustment. The queued installed restart also
   exercises the signed settings route and checks that it advances main
   while preserving the caller's private revision. See
   `p5-native-workspace-config-final.log`.
   Base `abca687` reads native invocation evidence and message trails from
   the conversation tables directly. The test builds only those tables and
   verifies the original statement and invocation trail. The existing
   legacy reader shares the same message and trail logic. All 36 projected
   Base tests passed. The full Base type checks are queued with the paired
   release. Restricted memory recall still calculates visibility from the
   legacy workspace history. See `p5-trajectory-evidence-base-final.log`.
   Host `b10352e0c` and Base `793f33b` index the exact native commit at
   each protected publication and retry the current protected commit on
   reopen. Legacy commit recall rebuilds its index from published ancestry
   and removes older entries for private saved commits. The 73 focused
   Host tests and 43 projected Base tests passed. The installed settings
   publication now also checks recall of that exact native commit. See
   `p5-native-commit-recall-host.log` and
   `p5-native-commit-recall-base-final.log`.
   Host `e17cef443` and Base `e8de701` backfill older native commits in
   pages bound to an immutable protected revision. Each page and its
   progress marker commit together. After interruption the host resumes
   that revision, then indexes the additions through the latest protected
   head. All 88 focused Host and 44 projected Base tests passed, including
   interruption and atomic rollback. See
   `p5-native-commit-backfill-host.log` and `p5-native-commit-backfill-base.log`.
   Host `b5af4d250` and Base `f917e3d` scope conversation recall to the
   verified caller's own channel and exact invocation evidence in their
   working revision and the protected shared revision. The existing RPC
   carries the host's authenticated scope. Message matching uses exact
   log, head, and message coordinates before ranking. The native test
   succeeds with the legacy command visibility table removed. This pair
   passed 63 Host orchestration tests, 177 Host RPC tests, and 14 projected
   Base tests. Conversation recall still needs a real agent check across
   two independent participant copies.
   See `p5-native-recall-host.log`, `p5-native-recall-rpc.log`, and
   `p5-native-recall-base.log`.
   Host `4b2e6731d` and Base `ab47bc7` retire the legacy commit recall
   projection atomically with the first complete native backfill. An index
   completed by the prior release retires old rows on its next read. An
   interrupted initial backfill preserves legacy published recall until it
   completes. All 15 Host history tests and 83 projected Base tests passed,
   including rollback and recovery after removal of the old commit tables.
   See
   `p5-native-commit-projection-host.log` and
   `p5-native-commit-projection-base.log`.
   Base `26055fe` records the participant reservation and its owner in
   one local transaction. An injected identity write failure previously
   left a reservation the retirement path could not identify. The new
   test verifies rollback before retirement. A separate restart test on
   the storage fixture loses the fork reply and advances shared history.
   Three repeat requests still recover the original fork and one grant.
   All 14 focused Base tests passed. See
   `p5-participant-reservation-before.log` and
   `p5-participant-reservation-after.log`.
   A separate E19 Linux process-loss run bundled exact Host `4b2e6731d`
   and Base `26055fe`. It killed the runtime with SIGKILL after the native
   storage fork finished and before the reservation reply. After restart
   it advanced shared main and retried the reservation three times at once.
   The same physical working copy kept the exact original commit and event
   rows, while the canonical store held one active allocation and one
   grant. The fixture uses a local adapter around the native fork call.
   See `p5-participant-reservation-native/evidence.log` in the private
   state directory. The build and process driver are in the same directory.
   Base `9323953` reclaims all grants for an exact participant transition
   when recovery acknowledges its applied publication. A regression test
   first reproduced two grants surviving acknowledgement. All 15 focused
   Base tests passed after the fix. The E19 Linux fixture also killed the
   runtime after the native push advanced main and before the push reply.
   On reopen it recovered the approved event from the actual main hash
   with no second push, reclaimed both temporary grants, and preserved the
   participant's regular grant. The fixture uses a local carrier. See
   `p5-publication-grant-native/evidence.log` and
   `p5-publication-grant-after.log`. The installed upgrade is queued
   against the current paired release described below.
   Production bootstrap in Host `238eb1890` now connects native workspace
   retirement to the runtime's storage reclamation queue. The host passes
   the exact retired copy to that queue after canonical archival succeeds.
   All 74 focused Host tests passed. The native installed check provisions
   a fresh E19 workspace,
   saves private bytes, restarts E19, verifies the owner's copy and
   independent reader, then runs publication and configuration checks.
   It replaces the queued legacy retention and upgrade checks. Earlier
   migration observations record their original runs. See
   `p5-retirement-storage-bootstrap-after.log` and
   `verify-staged-native-participants.sh`.
   Base `04ccc83` stages fresh template placements and installation
   provenance in a single native Dolt revision. Staging leaves shared
   main at its initial head. Installation requires the exact staged
   revision and input digest. The E19 Linux fixture verified the template
   file placement, rejection of a changed input or event, publication
   into main, and a normal restart. See
   `p5-native-fresh-root/final-evidence.log`. Host `b78252080` now uses
   that path for fresh template workspaces. It compares the staged files
   with the prepared template, admits the exact root through the existing
   protected creation gate and reopens native history directly after a
   normal restart. All 89 focused Host tests passed. See
   `p5-native-fresh-root-host-final.log`.
   Host `98fc210d5` removes automatic imports of old workspace history,
   panel state and workerd storage from production startup. Fresh and
   seeded workspaces attach native history before becoming ready. A
   workspace with old saved state and no native owner fails before a
   native history cell is reserved. Existing import code and historical
   data are retained outside the startup path. The 84 focused Host tests
   and nine runtime bootstrap tests passed. See `p5-native-only-bootstrap.log`
   and `p5-native-only-runtime-bootstrap.log`.
   Host `c03d41647` deletes the unused legacy activation path and requires
   attached native history and a protected main before startup completes.
   The 77 focused tests passed, along with a targeted check of the new
   activation guard. See `p5-native-activation-cleanup.log` and
   `p5-native-activation-guard.log`.
   Host `2fd629be5` attaches native history without replay ownership or
   retired-source records. Base `9168c9d` binds participants to their existing
   Dolt branch and starts a new branch from shared main. The Base working-copy
   test reopens a private commit, verifies isolation, and merges and publishes
   through the approved shared-history path. All 90 focused Host and 15 Base
   tests passed. See
   `p5-native-context-binding-host.log` and
   `p5-native-context-binding-base-final.log`.
   Host `2e3e713c3` and Base `23a33cf` remove the uncalled history replay
   driver, worker methods, and staging code. Runtime context listing reads
   native registrations and excludes retired contexts. All 121 focused Base
   tests passed, including a context-list check after reopening. See
   `p5-native-remove-history-replay-base.log`.
   Host `1f323560b` routes public VCS mutations through the authenticated
   participant's Dolt history. A request before native attachment reports
   that history is unready. Tests covered lifecycle ownership, causal
   admission, exact publication approval, and rejection before attachment.
   All 64 focused Host tests and the authority ledger check passed. See
   `p5-native-vcs-mutations-host.log`.
   Host `262de1521` uses the isolated native copy for workspace settings
   and reads no-op previews from protected main. Tests verify that settings
   publication carries the original approving caller, rejects invalid prepared
   edits before writing, and preserves a publication error if cleanup also
   fails. All 20 focused tests passed. See `p5-native-config-writer-host.log`.
   Base `a57fd11` reads commit recall from the protected Dolt publication
   index. Rebuilding conversation projections preserves the published
   documents and their index marker. The 21 focused recall and
   participant-history tests passed. The current Base integration branch
   includes it. See `p5-native-commit-recall-only-focused.log`.
   The representative fresh-history size check used full file placement
   metadata from 135 repositories and 5,135 files. The E19 Linux fixture
   staged the first native commit in 1.08 seconds and verified every
   placement. It exported 3,773,680 and 3,877,908 bytes for two working
   copies. Their allocation requests completed in 724 and 654 ms, and
   saving different contents in each copy left shared history unchanged.
   These timings include the local allocation request and use a local
   adapter for the physical fork. See
   `p5-native-fresh-size/evidence.log`.
   The same 135-repository, 5,135-file fixture passed on Base `298eae9`
   with Ubuntu x64 runtime `61862f2`. The fresh projection matched every
   input placement. Repeat allocation returned the same copy for each
   participant, and the trace recorded exactly two physical exports.
   The participants' independent edits left protected main unchanged.
   Staging took 653 ms. The two allocations took 334 and 397 ms and
   transferred 3,827,714 and 3,934,428 bytes. See
   `p5-native-fresh-size-current-release/evidence.log`.
   The native E19 concurrency scenario publishes from two independent
   work cells at once. Before approval, each read grant refuses push.
   Exactly one concurrent push succeeds. The other cell resolves one
   conflict, preserves both clean changes and publishes the merge. The
   scenario verifies the exact two-step progression of shared history and
   the idempotent repeat. It passed alongside the remote-operation restart
   scenario on the E19 Linux candidate. This verifies the runtime part of
   the gate. See `p5-e19-participant-push-repeat.log`.
   The concurrent publication check and authenticated two-node Dolt carrier
   also passed on the exact Ubuntu x64 runtime `61862f2`.
   See `p5-native-publication-61862f2.log` and
   `p5-native-dolt-p2p-61862f2.log`.
   The installed upgrade now ends with the approval-service race in
   `check_installed_participant_race.py`. Two authenticated participants
   save distinct revisions from the same shared event and submit both
   publications while the first is pending review. The driver verifies
   the exact file digests on the approval card, the stale rejection, one
   recorded merge decision, and the exact bytes from an independent
   observer. Both winner orders passed the driver simulation, recorded in
   `p5-installed-participant-race-driver-check.log`. The actual installed
   fresh and normal-restart run uses dedicated checkouts. Packaging
   waits for the loopback package registry and sufficient memory.
   The model-free 10,000-turn benchmark started as
   `st_9e007686d40145a6827546df05aedd9e` under
   `vs-verify-main-fork-benchmark3.service`. It runs the exact paired
   release and E19 Linux candidate with one lane, a 12 GiB memory cap
   and zero swap. It checked for 18 GiB available before launch. The
   supervisor logs and sampler use the `p4-main-fork-benchmark3-` prefix
   in the private state directory.
   At 14:13 CET, the benchmark's durable mission records contained 1,012
   successful model-free turns and one executing turn. The runner remains
   active with a 24-hour case budget. Its memory peaked at 5.91 GiB with
   zero recorded memory kills. Fork timing requires the completed validator
   result. See `p4-main-fork-benchmark3-durable-progress.json`.
   The benchmark's last durable turn was 1,963. That mission succeeded and
   its channel completion matched the canonical trajectory event and stored
   eval result, while the benchmark stopped advancing in observation. The
   test was cancelled through its normal command and the owned instance
   stopped. The accumulated results and memory samples are retained.
   System-testing `d19b0b0` verifies each mission's exact invocation against
   paged channel history and the canonical stored result. It rejects missing
   or foreign evidence instead of waiting on the live message view. All 17
   focused tests passed. The corrected benchmark is queued after the native
   installed and Linux artifact checks and will run the 65-turn soak before
   the 10,000-turn case. See `p4-main-fork-benchmark3-before-cancel.json`,
   `p4-main-fork-benchmark3-result.log`, and
   `p4-native-durable-receipts-final.log`.
   GitHub accepted the celld fork update at `23c4855`. The artifact
   workflow dispatch still returns HTTP 422, `Actions has been disabled
   for this repository`, even though the repository permissions report
   Actions enabled and the workflow reports active. The working E19 Linux
   candidate needs glibc 2.39. A smoke run on the release's Ubuntu 22.04
   baseline rejected it, while the previous pinned Linux artifact started.
   The first Ubuntu 22.04 build stopped when the DoltLite build script tried
   to fetch into the read-only source checkout. The checksum-verified
   DoltLite 0.51.11 source is now staged in the writable build directory.
   The retry, `vs-build-e19-linux-x64-retry.service`, built exact fork
   revision `23c4855` and ran `celld --version` on Ubuntu 22.04. The
   binary's maximum glibc requirement is 2.34. The Linux x64 payload and
   manifest are in `p5-e19-local-artifacts/celld-linux-x64`, with SHA-256
   `f5a09643715e0d4e799fac628e3e88394264866e03f17a87ef60f596b777802b`.
   See `p5-e19-linux-x64-build.log` and the service journal.
   The hosted workflow again returned HTTP 422 for this exact remote
   commit. See `p5-e19-hosted-artifacts-current.log`.
   A later read confirmed that the workflow is listed as active and the
   repository reports Actions enabled. The E19 branch head has no check
   suites. The workflow file is absent from the fork's default branch,
   which GitHub requires for manual dispatch. See
   [GitHub's manual workflow requirements](https://docs.github.com/en/actions/how-tos/manage-workflow-runs/manually-run-a-workflow).
   The first checkout profile on that executable recorded p95 of
   13.44 ms when quiet and 20.21 ms after a local write, with 10,000
   retained local rows. The separate syscall trace recorded successful
   disk syncs in both phases. The write phase exceeded the 20 ms gate by
   0.21 ms. A repeat without the installed fixture running recorded
   18.84 and 24.73 ms, respectively. Both trials saw host disk pressure.
   The release gate remains open. See
   `p5-native-checkout-profile/jammy-checkout-gate.json` and
   `jammy-repeat-checkout-gate.json` in the same directory.
   A small probe linked the exact release's DoltLite library and retained
   both committed file states and all 10,000 local rows. Wrapping the
   checkout sequence in one SQL transaction saved sync calls in a traced
   run but did not improve the interleaved timing samples. The production
   checkout is unchanged. See `e19-checkout-transaction-probe/interleaved.log`.
   The same E19 binary was measured again after the reboot, with no other
   project scopes active. The run recorded 15.21 ms p95 when quiet and
   22.89 ms after a local write. Post-write p95 was 23.17 ms for checkout
   to the alternate branch and 12.20 ms for checkout to main. The trace
   observed disk syncs and the host saw I/O pressure during the trial.
   The 20 ms gate remains open. See
   `p5-native-checkout-profile/jammy-isolated-checkout-gate.json` and
   `jammy-isolated-checkout-latency.json` in the same directory.
   A focused DoltLite probe moved the pointer write before the switch
   from main and grouped both in one transaction. It retained both
   committed file states, all local rows, and the branch selected on
   reopen. The control direction also varied substantially across the
   two runs, so the trial does not establish a production speedup. See
   `e19-checkout-main-pointer-probe/compare.log`.
   A paired trial now alternates the baseline and transaction on two
   open fixtures. Its memory-backed smoke run verified both file states,
   all 10,000 local rows, the selected branch on reopen, and rollback
   when the requested branch does not exist. In three same-disk trials,
   grouping checkout from main reduced p95 for that direction from
   15.46–17.12 ms to 7.66–10.10 ms. The return direction moved from
   7.76–8.78 ms to 16.28–17.91 ms. The probe now also groups checkout
   back to main with the pointer update, and passes the same smoke
   checks in memory. Its paired disk run is queued. See
   `e19-checkout-main-pointer-probe/paired-disk-results.log` and
   `e19-checkout-main-pointer-probe/paired-bidirectional-tmpfs.log`.
   The bidirectional disk run reduced p95 toward the alternate branch
   from 14.14–14.89 ms to 7.28–7.31 ms and kept the return to main at
   7.21–7.27 ms. The probe verified normal rollback and confirmed that a
   successful switch followed by rollback must also restore Dolt's
   connection-local branch selection. Celld `61862f2` implements that
   transaction and restores the selected branch on error. All 20
   non-ignored Dolt cell tests passed. The exact Ubuntu 22.04 artifact
   passed the 20 ms checkout gate with 10,000 local rows. Each phase
   measured 200 checkouts, with p95 of 11.49 ms when quiet and 16.56 ms
   after local writes. The separate trace recorded 112 and 219
   successful disk syncs. Its SHA-256 is
   `83fd90a39e266a798a5922e702746addb41f1c55b349072bb8d976c2f9fe7ac4`.
   See `p5-native-checkout-profile/jammy-transaction-checkout-gate.json`
   and `jammy-transaction-checkout-sync.json` in the same directory.
   The first full installed retry passed the Host compilation check and
   stopped at the missing `node-pty` host dependency. The repository's
   repair command rebuilt it. The current Host passed 38 artifact
   contracts, the executable and import smoke checks, three native host
   contracts, and all 29 workflow checks. See
   `p5-installer-pty-failure.log`, `p5-current-host-artifact-contracts.log`,
   and `p5-current-host-workflows.log`.
   Host `13062f4bf` updates the Dolt and provenance plans to use the normal
   participant release gate. Release preparation then passed all four
   Host type configurations and stopped at the Base type check.
   Base `95c3fde` fixes the idle import, strict record accesses, and
   the oversized journal fixture, which now uses a valid profiling
   receipt. All four Base type configurations then passed. System-testing
   `b78c5f7` narrows the receipt reader to the RPC operation it uses.
   All 17 focused benchmark tests passed. `911fdd5` also checks the
   invocation payload variant before reading its start fields. See
   `p5-installer-base-types-failure.log` and
   `p5-installer-invocation-payload-types-failure.log`. The retry prepares
   Host `13062f4bf`, Base `a57fd11`, and System-testing `911fdd5` to run
   against the verified Ubuntu 22.04 binary. Those exact sources passed
   their Host and userland type configurations and all 17 benchmark tests.
   Packaging finished and wrote the exact three-revision release receipt.
   On the resulting installation, a signed participant made a private
   commit and a later draft. After a normal restart the owner reopened
   both exact revisions and their bytes. A second signed participant
   could not read either private revision, and protected main stayed
   fixed. See `p5-native-current-main-release-prepare.log` and
   `p5-installed-native-participants-evidence.log`.
   The separate settings check reached a visible publication review and
   was still building affected projects when its six-minute fixture
   deadline elapsed. `vs-verify-native-install-review-resume.service`
   reuses the same installed release and persistent workspace. It checks
   that the interrupted review left protected main fixed, runs the
   participant publication and merge exercise first, then finishes the
   settings check with time for the cold build.
   The first stop command also hit its fixture deadline while the
   workspace still owned a package install. Its registry had shut down,
   and the install logged connection refusals. The cleanup supervisor
   restored the same local registry. The resumed participant and pair
   launchers now keep their registry up until the managed workspace
   scopes have retired. See `p5-native-cleanup-registry-refusal.log`.
   The old supervisor retired after reporting a build-cleanup error.
   The resumed test reopened the same workspace and verified the same
   protected main before starting the two-participant publication check.
   It exercises saves, review, conflict resolution, and merged bytes.
   The settings check recognizes the review for the exact
   `meta/vibestudio.yml` change with an empty additional-grant selection.
   The installed race published one of two independent saves, refused
   the stale publication, then published the resolved two-parent merge.
   The independent observer failed to read the file adopted from the
   other parent. On the retained observer database, SQLite chose an
   unconstrained `dolt_commit_ancestors` scan rooted at local refs.
   Base `7c6d40c` binds the exact fetched revision before scanning its
   parents. The old query reproduced the missing merge on that database.
   The fixed query selected the parent that supplied the file and its
   exact content origin for all three files on all three retained copies.
   The original-save lookup had the same issue for a save outside the
   observer's local refs. Base `bd30de6` corrects it. On the retained
   observer database, it finds both participants' original saves. See
   `p5-installed-native-merge-origin-observer-failure.log` and
   `p5-installed-merge-origin/all-merged-file-origins.log` and
   `p5-installed-merge-origin/first-save-attribution.log`.
   `vs-verify-native-install-fetched-ancestry.service` builds both fixes
   with Host `13062f4bf` and System-testing `911fdd5` and runs the same
   signed workflow on a fresh installation.
   That installation passed the private commit, draft, normal restart,
   and independent-reader checks again. During the first publication
   review celld exited with its node-lease watchdog code after a renewal
   stalled. The test client received an incomplete HTTP response and
   the review ended without advancing protected main. The same machine
   later logged global out-of-memory kills and rebooted. No heavy
   verification scopes remained active after the reboot.
   The sealed release and persistent workspace are retained.
   `vs-verify-native-install-after-reboot.service` waits for 22 GiB of
   available machine memory and low memory pressure. It reopens the
   same workspace, checks protected main against the recorded receipt,
   and verifies any retained private seed bytes before submitting a
   new review. It stops only its own workspace scope if available
   machine memory falls below 10 GiB during the test. The resumed
   run verified the same protected main, recovered the private seed
   files byte for byte, and approved their publication. Both
   participants then saved separately from that shared revision.
   Bob published first and the stale Alice publication was refused.
   Alice resolved the conflicting file and published the two-parent
   merge through review. The independent observer read the resolution
   and both clean changes byte for byte at the resulting protected
   revision. The separate settings publication reached the production
   build gate, which failed typechecking the system-test runner.
   Its materialized dependencies are missing relative imports into
   sibling skill and extension directories, including imports of type
   declarations. The runner builds from the complete checkout.
   See `p5-installed-native-merge-origin-workspace-config.log`.
   Base `fbf33e6` and the System-testing branch ending at `7b93b97`
   declare the missing compiler inputs in the package graph. The exact
   installed-source closure check selected the owners of all 37
   cross-project type imports. The isolated compile and a fresh settings
   publication remained to verify. The fresh installed retry completed
   all 28 checks and reached a different set of build failures, including
   mobile compilation, a missing runtime type export, and authority
   declarations. The protected ref stayed at its earlier event. See
   `p5-native-settings-type-dependencies-workspace-config.log`.
   Host `924d067f3` compares candidate service bindings with the
   currently protected metadata using exact provider manifests. This
   avoids rebuilding unchanged consumers for an ordinary workspace
   setting. Changed and removed providers still trigger their consumer
   checks, and new worker classes still require schema verification.
   All 44 focused build-scope and publication tests passed. On that Host
   revision and the current Ubuntu runtime, the installed call changed
   `defaultRepo` to `meta`, advanced protected main, and kept the
   participant's working revision. After shutdown the retained recall
   index linked the exact published event
   `workspace-event:d904265053a7af5363a627b73a2be08ab8d3e18f2708fa04cae8e0accbd665a8`
   to Dolt revision `98b72f105cf7d10cd8c5d162d81e0a0fe89dcb39`.
   A read-only retry reopened that exact workspace, verified `meta` from
   the live configuration API, and recalled the same event and revision.
   The same retry reopened the participant's exact working copy and
   verified both saved and draft bytes. See
   `p5-native-scoped-settings-workspace-config.log`,
   `p5-native-scoped-settings-retained-proof.json`, and
   `p5-native-scoped-settings-retained-rpc.log`. The fixture stopped
   normally.
   The participant merge remains independently verified. See
   `p5-installed-native-merge-origin-resume-evidence.log` and the
   `p5-installed-native-merge-origin-race-*` logs.
   After that test, `vs-verify-p5-p6-installed-pair-current-chronolog.service`
   uses the same Host and template revisions to prepare a release with
   the updated Chronolog package for a fresh two-workspace installation.
   It verifies separate signed identities,
   shared group writes, the protected revision and exact published file
   bytes in both workspaces. Results go to
   `p5-p6-current-installed-pair-evidence.log`. The first run installed
   the signed seed on the second workspace and stopped when its command
   line review could not parse the approval JSON. The resume reopened the
   same archived seed, verified its persisted receipt and let the managed
   doctor complete the creation review. The signed group checks continued
   from that workspace. Both signed participants wrote from their own
   nodes and read the resulting rows. An update to another member's row
   was rejected. The seeded workspace then published a file revision
   through review. The original workspace's protected main matched the
   signed group tip and resolved the exact revised file bytes. The
   installed pair passed and shut down normally on Host `13062f4bf`,
   Base `bd30de6`, System-testing `911fdd5`, and the sealed Chronolog
   `f38b9d5` package. See `p5-p6-current-installed-pair-evidence.log`,
   `p5-p6-current-installed-pair-observe-remote-head.log`, and
   `p5-p6-current-installed-pair-final-stop.log`.
   The durable benchmark used the exact receipt for the same Host,
   template, and updated Chronolog package revisions. Its short case
   launched the first model-free agent turn, found its durable
   completion, and failed the separate trajectory lookup. The retained
   run record contains the matching invocation and the result blob
   reference directly in the durable channel page. System-testing
   `7b93b97` reads that result from the exact channel receipt and checks
   its invocation, result marker, and turn number. The focused checks
   passed all 17 tests. See `p4-native-durable-receipts-soak-detail.json`
   and `p5-p4-publication-types-and-native-receipts-check.log`.
   The installed retry found the first turn and rejected the return value
   it received. The retained channel event contains the result's digest.
   The verified blob has the expected marker, turn number, and storage
   sample. The upload is private to the writer's task grant, while
   replay hydrates the value under the reader's authority. The channel
   needs durable ownership of the values its own committed events carry.
   See `p4-native-durable-channel-receipts-inspect.json` and
   `p4-native-durable-channel-receipts-soak-detail.json`.
   Base `65c2cb4` writes the result bytes in the same chunked channel
   record as their event. Replicas and forks can hydrate those bytes
   through the channel's own replay. Workspace-level observation still
   receives the compact envelope, and unrelated blob uploads keep their
   existing access checks. All 170 focused channel tests passed,
   covering exact forked bytes and atomic publish failure. The installed
   retry verified 20 turns before the 21st mission settled ahead of its
   channel delivery. Both the start and completion were present in the
   retained channel. System-testing `5845c28` joins that exact delivered
   invocation before reading the durable replay window. All 17 focused
   tests passed. The next installed run verified all 65 distinct turn
   receipts with zero model calls. The normal conversation fork then
   tried to build `workers/pubsub-channel` from the old context store,
   where the fresh native fork's files were absent. Host `56af4331e`
   binds cloned runtimes to the exact participant revision already
   retained by the admitted fork. It composes that revision once and
   uses its content and semantic identity for each cloned runtime.
   All 213 focused tests passed. The short case
   `st_30d2bc058fa6476da4315e4e771901a4` passed on the sealed release
   with `56af4331e` in 313 seconds. It verified 65 durable model-free turns,
   compaction on the normal schedule, and a normal conversation fork.
   The 10,000-turn run `st_78043a02fcc448f990cce69d3f0a0e4d` started on
   that same release. See `p4-native-clone-source-soak-result.log` and
   `p4-native-clone-source-benchmark-start.log`.
   See `p4-native-published-channel-results-inspect.json`,
   `p4-clone-native-source-check.log`,
   `p4-channel-stored-result-check2.log` and
   `p4-native-owned-channel-results-soak-detail.json`.
   The first 65-turn attempt used Host `924d067f3`, Base `65c2cb4`, System-testing
   `5845c28`, Chronolog `f38b9d5`, and Ubuntu runtime `61862f2`. It retains
   each case's run ID before waiting, so the
   first failure can be inspected while its managed fixture is still
   running. The supervisors stage these checks serially with the same
   22 GiB start threshold and 10 GiB machine reserve.
   Host `22087f52a` removes the managed filesystem route to the old
   workspace store. Calls wait for native history while it attaches
   and fail if the participant copy is unavailable. All 134 focused
   filesystem and workspace orchestration tests passed. Base
   `f03b389` removes conversation recall's use of the old context
   graph. The caller's own channel and the host-attested native
   causes now determine which messages can be recalled. All 11 focused
   privacy and recall tests passed. Both commits are in separate
   local candidates while the sealed settings and benchmark fixture
   runs on its original revisions.
   Host `a537fca57` removes the unused legacy bootstrap client and makes
   mirror and caller-scoped content reads require verified participant
   history. Mirror callers wait for the native attachment during startup.
   All 101 focused orchestration, source dispatch, and mirror tests passed.
   See `p5-native-content-only-check.log`.
   Host `cc0e91e57` and Base `4d35e67` remove the retired workspace
   initializer and its six RPC methods. The canonical Dolt owner
   validates the exact prepared root directly. The old initialization
   table and freeze/replay flags are gone. The Base schema version is 67
   for fresh workspaces, with no migration step. The 31 focused Host
   tests, runtime client manifest check, and lockfile check passed.
   Host `d63af2bd4` and Base `902241e` also remove the obsolete
   context-fork RPC and its handler. The fresh installation is queued
   behind the benchmark in `vs-verify-native-root-bootstrap.service`.
   See `p5-native-root-host-check.log`.
   Host `f8b77922b` stops conversation and subagent forks from also
   creating a source directory in the old store. The native participant
   fork chooses the revision and managed filesystem access materializes
   it on demand. All 159 runtime-service tests passed. This is a later
   candidate than the active benchmark and queued fresh installation.
   See `p5-native-runtime-folder-demand-check.log`. Host `3fde24439`
   now provisions only writable scratch through
   `workspace.ensureContextFolder`. The shell extension already uses
   that path for terminal confinement, and Base `fc145af` checks the
   scratch-only response in its fixtures. The unused desktop folder
   adapter and the host's old source provisioning hook are removed.
   All 126 focused Host tests and the agent API reference check passed.
   The paired shell case remains to verify on a fresh installation.
   See `p5-native-terminal-scratch-check.log`.
   Host `c12da626f` and Base `15f2694` remove the old context lifecycle
   RPCs. Runtime registration, enumeration, and retirement now require
   the attached native history. The focused tests verify that registration
   waits for attachment, retirement waits for an in-flight registration,
   and a retired context cannot reopen. All 101 focused Host tests and
   the runtime client manifest check passed. The matched Base test and
   fresh installed verification remain. See
   `p5-native-context-lifecycle-check2.log`.
   Host `d88a7e022` and Base `4ee600b` remove the old content-retention
   RPC. The sweep reads private and canonical Dolt history at preflight
   and again before deletion. If a registered participant cannot report
   its retained content, the sweep stops. Protected main, execution
   source trees, and explicit blob retentions are still checked by their
   owning stores. The 77 focused Host checks passed across the two runs,
   including a native edit holding the content lock until its private
   draft commits. See `p5-native-content-retention-check.log` and
   `p5-native-content-retention-check2.log`.
   Host `a69a44680` uses participant history for caller-scoped build
   sources, worker discovery, workspace API docs, and recall. An
   unavailable history no longer admits private build sources. Direct
   service authority for a retained runtime comes from the exact build
   it is running. All 113 focused Host tests passed. Installed verification
   of the combined candidate remains. See
   `p5-native-caller-build-source-check.log`.
   Host `574facebc` and Base `993acc3` remove the public VCS fallback
   to the old store and its reference visibility RPC. Public reads and
   writes wait for native history. An extension relay with a verified
   signed reader uses that reader's working copy for the authorized read.
   The 115 focused Host checks passed and the final 35 VCS tests passed
   after strengthening the relay assertions. The native workspace graph
   returns the originating invocation coordinate. See
   `p5-native-public-vcs-check2.log` and `p5-native-public-vcs-check3.log`.
   Host `3fbcf5e27` connects direct transcript inspection and neighbors
   to the trajectory owner in Base `8204b73`. A caller can navigate its
   authorized transcript head. A workspace cause grants access to its
   exact invocation without granting access to the surrounding transcript.
   All 68 focused Host and schema checks passed, including isolation of
   adjacent transcript heads (`p5-native-transcript-host-final.log`).
   The Base suite's first attempt stopped at the 768 MiB scope limit
   during infrastructure preparation. The fresh installation supervisor
   now selects these commits and runs the focused Base transcript and
   build suites after the long benchmark. It also checks the installed
   path from a private edit to its recorded invocation and transcript.
   Host `84c85a79b` removes the unused legacy history transport and
   requires the canonical Dolt publication authority for each ref update.
   Base `61e4fb7` removes the legacy workspace history receiver and stops
   creating its tables in fresh schema 68. Transcript search reads only
   the log messages linked to admitted Dolt commands. Invocation
   diagnostics report their recorded events, and workspace commands
   and their causes are inspected through VCS. All 94 focused Host checks,
   the generated API checks, and the exact transcript search SQL passed.
   The scheduled install selects this Host and Base pair and includes
   template storage, search, reporting, and release-pair build tests.
   Base `d1b5d96` separates mutation planning, lineage reads, cursor
   formatting, record identities, and transcript schema from the old
   planner and store. The production receivers load 31 workspace source
   modules without importing the old workspace implementation. The
   source audit compared all 15 moved operation bodies and found no
   changes to their logic. The latest queued release selects Host
   `84c85a79b` and Base `c5e9070`. The Base commit deletes the
   retired planner, storage, and radix implementation, along with tests
   specific to those persistence tables. It retains the shared file and
   repository placement rules and moves transcript cases onto the
   active schema. A host-side smoke test ran the compiled Base edit
   planner and inspected and paged exact transcript rows in SQLite,
   including the check for a cross-channel link
   (`p5-retired-planner-source-smoke.log`). The queued native installation
   verifies that a writer can query and follow their private edit while
   a second participant who knows its exact ID cannot.
   Host `a23423db4` and Base `893bd00` remove GAD's private log hash
   chain and its wire fields. Appends compare the expected sequence
   and envelope ID, fork receipts retain the parent's exact envelope,
   and fresh schema 69 checks sequence gaps, fork boundaries and
   head pointers. All 52 focused Host checks passed. A fresh Base
   database passed the direct append, fork and integrity smoke
   (`p5-log-coordinate-base-smoke.log`). The queued release selects
   these commits with System-testing `fc948a9` and Google Workspace
   `769f815`, whose fixtures now use envelope coordinates. The Base
   and companion suites and installation run are still pending.
   Base `60941af` caches retained placement roots in local Dolt
   tables, using commit diffs for direct branch advances and rebuilding
   after a branch leaves the retained set or a cache row is missing.
   The index serves garbage collection. File authorization still checks
   the admitted history directly. The exact Linux runtime verified that
   the physical fork starts with empty local index tables, preserves
   prior live hashes, retains private changes only in that copy, drops
   roots unique to a retired branch, and restores a missing cached
   root. See `p5-content-root-index/evidence.log`.
   Base `298eae9` adds a bounded local index of stable state identities
   at exact Dolt tips. The Linux runtime verified that the same ID
   could resolve to different origins at two private tips, remained
   unavailable from shared main, and failed closed after a merge with
   both origins. A physical fork starts with an empty index, and
   a normal reopen retains correct results. See
   `p5-workspace-commit-index/evidence.log`.
   The queued installation uses Host `81726bc42` merged from main
   `5ca222112`, Base `7d4535f`, and the verified Chronolog `f7351ce`
   package. It pins the following template revisions.

   | Template | Main | Release candidate |
   | --- | --- | --- |
   | Base | `36906d0` | `7d4535f` |
   | Examples | `52be2b0` | `c3455fe` |
   | Google Workspace | `726cf99` | `8472240` |
   | News | `b53effc` | `a8a47b0` |
   | Personal | `d73c01c` | `953b4db` |
   | Spectrolite | `07bcdd6` | `07bcdd6` |
   | System | `c96c839` | `976e318` |
   | System-testing | `42d5c31` | `316f83f` |

   The merged pair passed runtime documentation and authority checks.
   It has 188 unique Natlang service names, 192 reviewed capability
   classifications, and 385 checked RPC methods across 16 Base Durable
   Object packages. Base's generated runtime and VCS references match the
   release contracts. The Personal fixture declares ordinary test authority.
   The 29 workflow tests also passed. See
   `p5-merged-release-final-contract-check.log` and
   `p5-merged-release-contract-check.log`.
   Host `aab9f8635` passed 23 focused source tests for private tree
   materialization, owned runtime jobs, and protected publication.
   Two additional tests built native subprocess artifacts from that
   revision and verified self-contained launch and binary data across the
   supervisor and disk worker. See `p5-merged-release-native-operations-source.log`.
   The queued preparation built infrastructure and reached the full Host
   type check. It caught mismatches in the workspace search result and
   trajectory provenance interface. Host `fd66aa59d` fixes those errors
   and their test fixtures. All four Host type-check projects and 46
   VCS and publication tests passed on that checkout. See
   `p5-native-host-types-and-vcs.log` and the archived first preparation
   log `p5-native-root-bootstrap-release-prepare-aab9f8635.log`.
   The updated preparation waits for 20 GiB available, caps the build
   at 9 GiB and its registry at 512 MiB, and retains the 10 GiB
   machine-memory stop guard.
   The next preparation passed the Host checks and reached the Base
   type check. Base `9829c21` fixes a shadowed function in native text
   revert and updates the tests for the current envelope coordinates.
   Host `81726bc42` exposes installed Natlang module mounts in the
   temporary TypeScript projection. A focused probe resolved all three
   previously missing type modules and skipped uninstalled companions.
   TypeScript compiled the three affected source files through those
   mounts. The official paired test runner passed all 11 participant
   history and trajectory evidence tests on the same release revisions.
   See `p5-mounted-type-module-compiler.log` and
   `p5-current-release-focused-pair.log`.
   The complete Base type check passed for workspace, integration, and
   both mobile configurations. Two older fixtures still used a fake
   invocation digest and a removed visibility table. Base `7d4535f`
   updates those fixtures. All 148 semantic control, recall, and protocol
   tests then passed. The release queue now uses that revision. See
   `p5-native-root-bootstrap-release-prepare-9829c21.log` and
   `p5-native-fixture-retry.log`. The earlier type failures and mount
   probe remain available in
   `p5-native-root-bootstrap-release-prepare-0e5f782.log` and
   `p5-natlang-types-probe.log`.
   Preparation passed the contract and type checks, 159 Base tests and
   three System-testing fixture tests. Two Gmail fixtures still expected
   a temperature field that the Codex API omits. Google Workspace
   `8472240` corrects those assertions, and all 56 Gmail tests passed.
   The only change to that candidate is the test file. The retry retains
   those verified results and runs the remaining tests and packaging.
   See `p5-native-root-bootstrap-release-prepare-before-gmail-fixture-fix.log`,
   `p5-gmail-native-fixture.log`, and
   `p5-native-root-bootstrap-current-templates.tsv`.
   A separate run on the exact candidate passed the 10 trajectory,
   workspace search, and problem-reporting tests. Its build fixture hit
   the diagnostic run's smaller Node heap and the combined run timed out
   during the scaffold suite. The full preparation queue still runs
   both build suites with its release memory settings. See
   `p5-final-release-pair-tests.log`.
   Release preparation then completed for Host `81726bc42`, Base
   `7d4535f` and the pinned companions. The two build suites passed
   22 tests with the release memory settings. The first fresh
   installation passed doctor and loaded the exact protected workspace
   source. Its fixture expected a synthetic agent invocation behind a
   direct CLI edit. The follow-up fixture verifies that the direct command
   points to its exact private work unit. Agent invocation evidence is
   exercised in the separate system-test run. The fresh-install and
   two-workspace checks are queued to repeat after that run.
   The requested test sweep is managed by
   `vs-verify-cutover-release-tests.service`. Its pinned inputs, four
   model-free cases, four model-driven orchestration cases, a 30-case
   model-driven sample, and retained run receipts
   are under `cutover-release-verification/`. It runs the celld E2E
   suite, Testkit, the managed system tests, and the Vibestudio smoke,
   transport, and browser integration suites without overlapping them.
   The baseline sweep passed 51 celld E2E scenarios and 24 Testkit
   tests. Celld also recorded the known R2 ETag failure and skipped two
   scenarios that need separate live model providers. Four model-free
   managed cases passed. Four model-driven orchestration cases and the
   first model-driven sample attempts exposed the same startup defect
   before model execution, so the sample was cancelled. Host
   `182eda130` keys authenticated task copies by the admitted owner
   context. All 43 focused ownership and host orchestration tests passed.
   `vs-verify-cutover-owner-context-tests.service` packaged the corrected
   candidate and started a fresh managed workspace. All four model-free
   cases and all four affected orchestration cases passed. All 30 sampled
   cases finished in `st_222b46911394433cbae7526a4c2bf9a2`, with 20
   passes, two validation failures, and eight errors. The extension and
   terminal smokes, headless panel integration, Iroh E2E, and the 43-case
   Playwright suite follow. Evidence is
   under `cutover-release-verification/owner-context-rerun/`.
   That sample also exposed separate native copy and fixture defects.
   Host `bc24177bc` binds native tool invocations to their authenticated
   agent lifetime, keeps admitted owner contexts separate, and uses
   the authenticated agent's private source for delegated build reads.
   It also resolves the owner of the source when retaining a child
   context's fork. The focused Host run passed 222 tests and Host
   `e3d86ef60` passed the explicit orchestrator-fork regression.
   System-testing `2dc8b38` gives the seven affected private fixture cases a seeded
   independent copy, checks the protected head before querying its
   ancestors during cleanup, and corrects two filesystem test assumptions.
   The retained directory-removal trajectory passed the corrected validator
   without another model call. Host `e249573ff` runs the image extension
   smoke and headless panel integration from the published host build
   generation. Both fixtures previously looked for a retired loose
   `dist/server.mjs` path and could silently skip.
   The baseline image-extension smoke and all 12 Iroh E2E cases passed.
   The terminal smoke found the CLI's unstaged ownership reaper.
   Host `bbb3da7a7` stages and checks the reaper files beside the CLI.
   Host `435ed7b6e` routes the headless panel fixture's hosted execution
   requests to the recorded entity, and its focused integration rerun
   passed. The browser baseline passed three startup cases and failed two
   workflow cases before being interrupted. Its logs showed the desktop
   template lookup tried to open a participant before any work context
   existed. Host `161c49cde` and Base `6e17323` instead read the exact
   published event from canonical Dolt history for that trusted client.
   All 52 focused Host service and history tests passed. The browser
   suite will rerun in full on those revisions.
   `vs-verify-cutover-agent-lifetime-tests.service` packaged Host
   `161c49cde`, Base `6e17323`, and System-testing `2dc8b38`.
   The four core model-free cases passed on those pins. Three expanded
   integration cases tried to compare a source already in their target's
   history, and one provenance case read from the wrong fixture owner.
   System-testing `73b969d` verifies the committed and working integration
   decisions and runs model-free provenance reads from the fixture owner.
   The focused integration oracle passed. An additional eval-recovery
   case stalled after its deadline with many requests contending for the
   same filesystem scope. Host `967ca1413` reuses the admitted owner
   until retirement and reads the current revision for subsequent
   operation effects. All 108 focused filesystem and orchestration tests
   passed. The owned stalled instance was stopped through its normal
   management command.
   `vs-verify-cutover-recorded-history-tests.service` now runs the nine
   remaining agentic results on a fresh package, repeats the four
   affected model-free integration cases, runs the smoke and transport
   suites and full browser suite, then checks eval recovery and the
   focused fixtures. Evidence stays under
   `cutover-release-verification/owner-context-rerun/agent-lifetime-rerun/recorded-history-rerun/`.
   The fresh run isolated a shared failure in the file-search, atomic-patch,
   and binary round-trip cases. Their edits reached the server with a native
   causal parent, which the prepared-edit schema rejected. Host
   `7984307d8` uses the shared causal-parent schema and passed all 80
   focused history, orchestration, and service tests. The scoped test
   extension reached its verification command but its child process could
   not resolve the test adapter. Base `2738fa4` exposes the admitted
   dependencies to that child. The same nested projection ran three of
   eight tests successfully at the old pin and all eight at the new pin.
   The retained trajectories and focused rerun logs are in the same
   evidence directory.
   The other five agentic cases passed on the fresh candidate. The run
   `st_98b43faad0dc4a138d9d5253acdfe337` finished with five passes, four
   validation failures and zero execution errors, leaving 26 of the
   original 30 verified after including the directory-removal replay.
   The two workspace integration qualifications and provenance passed in
   `st_7d421a00698549bb96353ea4ee88d9e0`. The handoff qualification reached
   nine of its eleven fixtures before its default ten-minute deadline.
   System-testing `dc3a2d8` gives that specific qualification 15 minutes.
   The remaining four agentic cases, handoff qualification, and smoke
   and E2E runs are assigned to
   `vs-verify-cutover-native-mutation-tests.service` on Host `7984307d8`,
   Base `2738fa4`, and System-testing `dc3a2d8`. It retains results
   in the `native-mutation-rerun/` subdirectory.
   The package built successfully and all four agentic retries passed in
   `st_8e69b02f7d0846c1a95fa52a04d894c4`. This closes the 30-case sample
   across the original run, targeted retries, and retained replay. The
   installed catalog also confirmed the three deterministic Testkit
   system suites, which are running with the handoff qualification.
   The first Testkit system run showed that the browser connection
   opened by its driver closes when that RPC returns, before commands
   from the next call arrive. Base `4e9afe0` opens raw Testkit connections
   through the current caller's approved panel handle and keeps them
   in the scope that uses them. The transcript also exposed a distinct
   celld failure linking a worker whose entry file and sibling import
   different names from `node:path`. Celld `b50fb34` joins the import
   bindings for the full module graph before registering builtins. The
   new `loader-external-imports` E2E case reproduces the missing export
   on `61862f2`. The next serial candidate runner is
   `vs-verify-cutover-panel-fixtures.service`, with logs under
   `native-mutation-rerun/panel-fixture-rerun/`.
   The 15-minute handoff run reached its eleventh fixture before timing
   out, so System-testing `f5fd5bf` budgets 20 minutes for that case.
   The image-extension smoke passed again, and the terminal smoke
   rendered its chrome before waiting indefinitely for its obsolete
   Ctrl-Space chord. Host `974a9971c` sends the installed client's
   default Ctrl-B chord and bounds the wait for an orderly exit. All 11
   focused terminal smoke harness tests passed. The older terminal
   attempt was stopped after recording the chrome milestone.
   The rebuilt `b50fb34` Linux binary passed both the new multi-module
   regression and the existing worker-loader tail case. On the same
   release, the image smoke, terminal startup and normal shutdown,
   headless panel, and all 12 Iroh cases passed. The 43-case Playwright
   run is active with a single worker. Its first two workflow cases
   expired while their cold workspace launch was still preparing.
   Host `62f6fcf95` gives those cases the standard launch-review
   deadline and enough overall time to run their actual assertions.
   The raw Testkit connection retry still failed because managed
   execution exposes no raw WebSocket or fetch. Base `a2de25d`, based
   on `2738fa4`, uses the runtime's existing stable panel automation
   session for viewport changes and evaluation. The focused unit and
   managed reruns plus the two browser retries are queued in
   `stable-panel-rerun/`.
   The interrupted browser run preserved three passes and five failures.
   The command-overlay test's snapshot showed the shell running behind
   the first-run reporting dialog while its fixture waited for an
   obsolete DOM marker. Host `a8dfa104a` uses the canonical launch gate
   to detect the running shell and attaches late shutdown logs on error.
   The continuation selects the other 40 cases in groups of 12 and 28,
   using two build slots with the same system memory reserve.
   The eval-recovery qualification returned a cell request-limit error
   while filesystem reads were asking for full status reports. Host
   `0c873eeb8` and Base `946f0a2` serialize this read per admitted context
   and fetch only the current semantic state. All 108 focused Host
   tests passed. The handoff qualification's latest setup failed when
   it reused a previously interrupted fixture, so it now gets its own
   fresh instance. System-testing `53f9702` also corrects the projected
   fixture test's expectation for a cleanup at the exact shared head.
   Both desktop pairing smokes stopped at System template validation.
   System `7917fc7` names the two unnamed skill packages. All eight
   bootable template checkouts passed the same validator after the fix.
   All eight passed full validation again on the current candidate. See
   `template-pairing-rerun/template-validation.tsv`.
   The full Linux E2E rerun on celld `b50fb34` passed 52 of 55 cases.
   The retained R2 ETag mismatch was expected. The Pi and OpenCode cases
   skipped because the live providers needed for those cases were absent.
   The stable release completed all 94 focused Base checks and all 179
   System-testing fixture checks. The live Testkit run
   `st_b13ebd49e3f642d5891e6fbf089b0d76` passed panel lifecycle and viewport.
   The chat transcript suite expired while waiting for the seeded
   conversation. The same behavior reproduced in an isolated panel with
   the transcript configuration. The retained transcript includes the
   panel snapshot, and `stable-panel-rerun/` holds the targeted probes.
   The fresh handoff qualification passed all eleven of its model-free
   fixtures in 19 minutes and 6 seconds, run
   `st_2da90f18ddf7427aa9142a6810fbf184`. The eval-recovery retry includes
   a fresh run of the exact chat transcript suite before the browser
   groups.
   The eval-recovery retry did not report a result before its outer
   deadline, and the queued chat suite did not run in that fixture.
   The original opening request was retained and replayed through the
   CLI. The browser panel joined and displayed it when the channel was
   initialized before opening the panel. Base `5f7af20` now retains the
   creation seed before starting the response-owned chat subscription.
   All 24 connection and ownership checks passed.
   The browser retry passed the test API startup case and reproduced two
   feature failures with an empty initial panel tree. The fixture template
   declared the chat panel and the runtime built its artifact after the
   panel initialization step had already run. Host `55a1df5b5` makes
   protected workspace configuration part of required startup so first-run
   panel initialization can read it. The 18 focused Host startup and
   reconciliation checks passed. The older browser run was retired,
   and `semantic-ready-rerun/` packages this Host and Base `5f7af20`
   together for the original transcript suite, all 43 browser cases, and
   the two desktop pairing checks. The installed two-workspace check and
   benchmark wait behind that runner.
   The rebuilt managed workspace passed its preflight and both live panel
   suites. In `st_59914271717148ae82f5f2312dc56d50`, the transcript
   displayed the original opening request, finished the eval bead, and
   rendered the deterministic reply. Its second case reached the fork
   action and found that the fixture agent still declared the default
   SQLite storage. System-testing `b33034f` gives that agent the same
   Dolt storage declaration as the product agent. Template validation and
   all three focused worker tests passed. The exact fork suite is queued
   on the next full candidate.
   The config-order correction reached the desktop, where the first two
   browser cases still found an empty panel list. Both fresh WorkspaceState
   cells had recorded an empty initialization marker after their revision
   had advanced during service activation. The focused regression shows
   that activating a worker before panel initialization causes zero seeds.
   Host `d73e0ff4b` bases this decision on actual slots and the retained
   initialization marker. All 129 focused state, startup, and panel
   reconciliation tests passed, including that regression. The older
   browser runner was stopped after the two failures. The combined runner
   at `panel-seeding-rerun/` checks the full product startup and the Dolt
   test agent together, then runs the 43 browser cases and both desktop
   pairing checks. The browser reporter records each failure when it occurs.
   In the combined managed run `st_12356cc7b46e412288997d67c1b28108`, the
   original transcript again passed and both the channel and its Dolt
   agent cloned. The fork navigation then tried to build the chat panel
   from the destination's independently owned working copy. Base
   `866a653` selects the calling panel's working copy for its executable
   source while the existing context admission controls which conversation
   it can open. All six focused panel authority and navigation checks
   passed. The exact live transcript is queued in `chat-fork-source-rerun/`.
   The desktop rerun recorded a seeded panel in both fresh private
   workspaces and activated the Personal chat. The first DX case reached
   the invocation diagnostic after its directory read. Host `d168902c5`
   removes obsolete command and effect fields from the E2E fixture and
   uses the current diagnostic types. The second was still waiting for its
   project adoption review while its fixture waited for launch. Host
   `61a4f0d47` resolves the reviewed creation intent before waiting for
   client startup. Both exact cases run again with the chat-fork candidate.
   The 13-case startup and command-overlay group recorded 10 passes.
   The browser panel worked with its optional data extension unavailable.
   The retained desktop shutdown log showed the workspace runtime slot
   being cleared while its already-admitted startup still needed it.
   Host `025bc8920` joins startup and closes its runtime before clearing
   the slots. All 39 focused desktop ownership checks passed. The other
   two failures concern reopening and promoting a saved Quickfire
   conversation. The 28 remaining browser cases are running.
   The Quickfire owner test reproduced a second reopen after an empty
   index refresh dropping the conversation the current view still had
   bound. System `18c3af0` keeps that exact live slot across a refresh.
   All 39 focused Quickfire tests passed. The cross-workspace browser
   test also waited for its client before resolving the pending creation
   review. Host `fa7fed95c` handles that review first. The queued
   `chat-fork-source-rerun/` release includes these pins and runs the
   transcript, both DX checks, the affected startup check, the full
   command-overlay flow, and the cross-workspace browser check.
   The remaining browser group also found outdated shell setup in the
   desktop chrome and startup approval scenarios. Host `92affd681` and
   `dbc5aa6ba` locate the hosted shell directly. Host `816836268`
   resolves first-run adoption before measuring panel geometry.
   Host `17bb572f7` uses the shared launch gate for the agent permission
   scenarios and gives each desktop launch its own prepared sources.
   The warm-launch test passed on that candidate. It reopened the same
   workspace with a new prepared source directory, observed no approval
   prompt, and called the app service through its existing grant.
   The first phone-width Electron case reached the replacement desktop
   client, whose sidebar and address controls superseded the old shell
   controls used by the test. Host `55ef27416` drives the current
   desktop UI and still checks panel selection, creation, terminal and
   chat sizes, and horizontal overflow. All 18 focused desktop
   shell checks passed. The phone-width sidebar browser case passed.
   Host `9d7826621` selects the address control for the exact Help panel
   after the next case found two matching titlebar controls. The remaining
   phone-width cases are queued with that change.
   The native Android pairing smoke booted its isolated emulator, then
   Metro stopped at a missing React Native workspace package link. The
   desktop checkout reused a shared dependency tree without the mobile
   workspace links. A dedicated Android checkout passed a frozen offline
   install and resolved the native polyfill from its exact package.
   Its full smoke retry built and installed the Android app and
   prepared all 150 runtimes against that checkout's host artifacts.
   The CLI paired as the first device. Its System agent attachment
   then waited for the workspace's history-index read for over ten
   minutes, before the test sent an invite to the phone. The owned CLI
   command was stopped and the smoke cleaned up its processes. The
   Android scope recorded zero OOM kills. Evidence is under
   `chat-fork-source-rerun/android-test/isolated-retry/`.
   Both desktop rebuild cases passed. The panel persistence test passed
   after each launch got its own prepared-source directory. Host
   `4df4beafb` makes the navigation test wait for the enabled launcher
   control and checks pending build state in the exact hosted shell. The 12 focused
   native-mobile app bar, panel tree, and layout tests passed.
   The Server Logs launch reached the launcher inside Personal.
   Server Logs, About Vibestudio, ad blocking, and permissions are
   units of the System template. Host `526b07ac6` opens the System
   fixture for both navigation scenarios. The next navigation screenshot
   showed that a partial text match selected a web search. Host
   `a9930611e` selects both installed units by the launcher's exact panel
   links. The same-panel navigation test passed through About, ad blocking,
   and permissions. Server Logs also opened and reached ready. Host
   `036e42385` gets the terminal browser fixture through the current
   launch gate. The rerun opened a usable terminal and missed the
   approval assertion, which still expected the old shell extension
   launch. Host `d05c24ff4` leaves the exact terminal-app launch permission
   unchecked at creation and verifies the interactive approval for the
   workspace's login shell. The next run opened the shell without
   recording a per-call prompt. Host `586e69c2c` confirms that the
   intended permission is present in the exact creation review and
   records whether the later launch gate selected that permission.
   The stricter fixture showed that the private workspace review had
   already settled. Host `86877879f` starts the terminal in a newly
   added project, where the fixture can leave the exact launch
   permission unchecked in a fresh review. All 72 focused terminal
   registry, install review, and authority checks passed.
   The remaining 28 browser cases finished with 6 passes, 17 failures,
   and 5 serial skips. The template-authoring report shows that the
   settings panel opened and the test timed out. Host `aaae362e4`
   uses that panel's readiness and the current hosted shell in both
   template flows. The two desktop pairing checks could not open
   Electron because Xvfb inherited the user's Wayland configuration.
   Their smoke entry points now select X11. Both template and pairing
   groups ran again at `template-pairing-rerun/`. Template authoring
   passed. The folder-creation case registered a separate workspace and
   reached a retired fingerprint assertion. Host `ed7a1d2bb` compares
   the recorded commit with the exact review checkpoint instead. The Iroh
   smoke reached the current per-pane titlebar, and the website smoke
   completed consent and connection. Host `36663208d` checks the current
   pane geometry. Host `7755aa43b` verifies a real SDK read and confirms
   access is revoked on disconnect. All 26 focused fixture and smoke
   contract checks passed. The folder retry proved the descriptor
   recorded the exact dirty checkout and reached the new project's
   install review. The approval card moved to another Electron surface
   while the test followed the old page. Host `879049994` follows the
   exact approval across those surfaces. The desktop pairing retries
   and managed Testkit provisioner rejected the package sealed for the
   earlier Host build. An initial build in the queued checkout also
   exposed missing workspace dependency links in the shared install.
   Its own frozen offline install resolved those links. Both smokes and
   Testkit will use the release sealed by that checkout.
   The website SDK build rejected the optional native canvas import
   pulled in by its HTML parser. Base `a0309eb` uses LinkeDOM's browser
   entry for the two HTML parsers. The full standalone SDK and its
   declarations built, and all 14 focused provider tests passed.
   Host `5ed2f662e` builds the SDK fixtures in a child process before
   launching the desktop. Both browser runs passed the SDK build. The
   invention case paired, connected, and recorded a reviewed publication
   to the shared workspace. Its preview was still reading the website's
   independent working copy. Host `8db7d7031` reads the exact protected
   main event and publishes revisions under the same approval workflow.
   Both preview regression cases failed before the fix. All 61 relay and
   website checks passed after it. The connection case correctly refused
   the call before consent. Host `f4006726f` exposes the structured error
   code in the test page so the assertion can see it. The scripted
   invention browser rerun passed. It rendered the reviewed artifact,
   received a real embedded reply, published the revision, and
   reconnected to the revised invention. The separate connection
   case reached the exact pending approval behind the first-run
   reporting dialog. Host `3f85bb14f` dismisses that dialog and uses
   the shared approval driver to open the exact request.
   The next managed run `st_002aca7c86c54a7f8566c125f6983a61` passed
   chat transcript, including opening the Dolt fork and returning to
   its parent, and panel viewport. Panel lifecycle rebuilt its panel
   and its supervision check caught `RPC call aborted by caller`.
   A focused fixture reproduced the reported error when closing before
   a channel subscription acknowledged. Base `b55782a` settles readiness
   as part of closing and stops the retired opener from reporting a
   recovery error. All 80 PubSub client tests passed. On this Base pin,
   the managed rerun again passed chat transcript and panel viewport.
   The lifecycle rebuild completed, and supervision still reported a
   cancellation as an error. The isolated replay applied the permitted
   approvals to a fresh session. All six lifecycle checks passed and
   supervision recorded no findings. Evidence is in
   `panel-lifecycle-approved-console-rerun/findings.json`. The exact
   catalog run `st_526b0240f58e4650b960c5dcf1a76e42` passed all three
   suites on Host `b163bebd7` with zero unexpected tool failures.
   The managed instance then exceeded its teardown deadline while a
   channel durable-work adoption call remained in flight. The runner
   retired its own scope. The deterministic result and shutdown have
   separate evidence in `sealed-desktop-sdk-rerun/testkit-result.log`
   and `sealed-desktop-sdk-rerun/testkit.log`.
   The desktop directory-read and causal-diagnostics retry passed.
   The screenshot and console-history retry opened its project and
   reached a chat panel that was still joining its conversation when
   the case timed out. Host `6e20fc7f4` uses the reviewed launcher
   caller and a Help panel for the same screenshot, blob round-trip,
   console history, and focus checks. The rerun reached the approved
   caller, whose launcher bundle had no dynamic runtime loader.
   Host `e09ccaa8d` declares the runtime import in the reviewed fixture.
   The next Electron run passed the screenshot, blob round-trip,
   console history, and focus assertions.
   The exact graceful-quit retry passed with no unexpected failures.
   The cross-workspace desktop check passed, including selecting two
   workspaces and calling the receiver in the other workspace.
   The Quickfire rerun passed six cases and reopened the command list
   while its first prompt was still queued. The trace in
   `quickfire-live-console-rerun/` records the same panel at dismissal
   and reopening, with no session yet in the durable list. System
   `8cb2a45` retains that panel's binding while the first prompt
   is queued. All 42 focused checks passed. They cover reopening the
   same slot, switching panels, and leaving an empty binding.
   Host `5381e14e0` waits for the actual conversation to contain the
   submitted prompt after reopening. The next desktop run reopened the
   conversation and exposed the agent join refusal. The receiver expects
   the authenticated code that created the agent. Host `f68556533` routes
   the initial join through that creator and preserves the initiating
   account's durable ownership. The focused regression failed before
   the fix. All 211 runtime and HTTP RPC checks passed afterward. The four
   focused browser cases passed in one session, including durable delivery
   after reopening and promotion into a chat panel with the same history.
   The four basic overlay interaction cases had passed in the full run.
   The focused report is in `quickfire-creator-rerun/browser.json`.
   Desktop startup reached the initial chat and the local test provider
   rejected the host's request to attest its model endpoint because the
   fixture still exported the retired method. Host `524d8413e` updates
   the fixture to the current server-only method. All 11 focused
   fixture and model-authority checks passed. The onboarding and scoped
   network browser cases ran on the package that preceded the agent
   creator fix. The first created the chat agent and did not observe its
   first turn. The second displayed the exact network approval, and
   its first channel read had no matching invocation yet. Host
   `574b480d2` observes the durable invocation before granting consent
   and still compares its identity with the completed invocation.
   The next run's screenshot shows the onboarding setup panel and
   completed reply. Its observer read the four-character chat label
   in the renderer, which resolved a different conversation. Host
   `58a72d762` reads the full channel identity from the actual chat
   panel's runtime state and inspects as that panel. The exact-channel
   onboarding retry found the prompt and the completed skill read.
   It also recorded a failed compaction because the scripted HTTP model
   tried to call a tool in the summary request. Host `67130909f`
   answers the tool-free summary request and carries forward completed
   fixture tool results. Both browser approval cases reached this same
   fixture failure. The three focused fixture checks passed, along
   with the earlier 122 filesystem, version-control, and approval checks.
   The next scoped network case passed and the onboarding journal
   recorded the finished reply, rendered setup card, and settled work.
   The onboarding case still timed out in its final chooser interaction.
   Host `02fa7ee70` clicks the actual workspace link through the native
   panel and reports overview rendering and chooser opening independently.
   That isolated retry passed, bringing the latest browser results to
   42 of 43.
   The phone-width desktop run passed the sidebar, address bar, new
   root panel, and background panel checks. Terminal stayed in the
   lease stage while the fixture waited without servicing later
   approvals. Host `8c7fe0d6e` drives approvals as the panel becomes
   ready and captures pending approvals on failure. The second run
   still showed the child terminal leasing with an empty approval
   queue. Host `b163bebd7` navigates the existing launcher slot to the
   terminal and verifies that exact source is ready before checking
   layout. The terminal and chat viewport cases both passed in separate
   browser runs.
   The geometry check passed on the next desktop run. Three other
   chrome checks reached the hosted shell and timed out. The logs
   showed repeated attempts to read unpinned panel metadata from the
   panel's separate working copy. Host `e75e3c13a` resolves unpinned
   metadata through the reader's scoped source. All 154 focused
   panel and lifecycle checks passed. Host `497b82b1a` keeps browser
   test RPC on the real shell transport and removes the Playwright
   binding setup that timed out. A focused probe checked both
   destinations, the exact caller, schema validation, permission
   failures, and listener cleanup. The private workspace catalog case
   passed. The custom chrome case was looking for a Browser Data
   permission that the shell launch review did not offer. Host
   `a2c83fdbe` verifies that the hosted chrome mounts, reuses its
   native surface, and routes through the workspace rows. The separate
   approval queue and unit admission suites cover that Browser Data
   permission, and all 84 checks passed. The custom chrome browser
   rerun passed. The file-copy case created
   a private review context and the shell's published catalog refused
   to resolve it. Host `12b3c6a46` reads through the destination
   filesystem. The next run confirmed the private working head and
   received an unknown-context error from the filesystem before that
   context hosted a panel. Host `8a43133e3` opens the real review panel
   and waits for that exact context before verifying the single file's
   bytes and hash and the unchanged protected main. The next run opened
   the review panel in System while the desktop remained on Personal.
   System `325f806` uses the same workspace switch as the sidebar and
   Host `e6bf35153` asserts the new focus directly after the review click.
   All 28 focused System shell, settings, and source-copy tests passed.
   The sealed desktop Iroh smoke passed native pairing, panel creation,
   workspace selection, and reconnection after a normal server restart.
   The website desktop smoke connected, read capabilities, and
   disconnected. Its console gate flagged Chromium's exact landing-page
   Web Share capability warning. Host `dd4cfc7bd` exempts that exact
   warning and preserves failures for other features, origins, and
   error levels. The full desktop smoke passed on the retry, including
   the final console check. Host `20390cb84` checks the repository-wide
   scope in the genuine website consent dialog. The browser case passed
   consent, file access, and revocation when the document was replaced.
   The terminal browser retry approved the exact command permission and
   exercised a live shell. It then exposed a rejected layout write.
   Base `b11daee` sends a JSON merge patch against the current panel
   state, including removals of optional fields when sessions close.
   All 29 terminal state, action, and panel state RPC checks passed.
   The next grouped browser run stopped this case in cold workspace
   startup before the terminal opened. The layout check has a separate
   fresh browser scope for its retry. Logs from the isolated attempt
   showed Personal starting near the gate deadline. Host `cfe6252ed`
   gives that cold companion startup its own additional time.
   That run opened a live shell and saved the expected three panes and
   focused session. On reload, the terminal rejected its own saved state.
   Base `93e83f9` normalizes the saved snapshot with the same rules as
   its reader and keeps the split positions supported by the UI. All
   30 focused layout and action tests passed. Host `b1aefef2a` retains
   the exact saved layout in the browser report before reloading.
   The isolated browser rerun passed layout restoration and the
   post-reload terminal checks. The aggregate is 43 of 43 passing
   across the full suite and the targeted reruns. The final terminal
   report is `profile-rerun/terminal-restore-rerun/retry-terminal-restore.json`.
   The folder checkout browser retry used the exact reviewed Git
   source and entered the newly approved workspace. Host `efb1f209e`
   checks the current launcher copy on that final surface. The rerun
   passed. The six-case browser retry passed the copy, scoped network,
   website connection, and local checkout cases. At this point the
   latest recorded results were 41 of 43 passing. The onboarding
   pointer and terminal layout cases have separate reruns.
   Android built and installed the APK and paired the command-line
   device. Its first isolated run stalled while attaching the System
   agent and ended before phone pairing. Host `5af80cd6b` shares the
   isolated remote server setup with the passing desktop smoke. Host
   `c2264baa2` pairs the phone with the existing account before the CLI
   attaches an agent for the later development workflow. The targeted
   retry reused the verified APK, paired the phone, and accepted its
   launch review. Celld reported insufficient memory headroom while the
   emulator and server shared a 12 GiB scope. The browser client never
   reported running, and the scope recorded zero OOM kills. The next
   retry gives the owned emulator and server separate scopes, with the
   same machine-wide memory guard. See `profile-rerun/android-smoke.log`
   and `profile-rerun/android-memory-admission.txt`.
   With separate scopes, Android paired again and the launch review
   resolved. The managed React Native build then failed to resolve
   `@kayahr/text-encoding/no-encodings` in the projected host API.
   Host `2314f0223` collects the npm dependency closure from the same
   trusted host manifests that supply the projected source. All 47
   dependency tests passed, including a host package with no pnpm
   workspace link and a dependency declared by a nested host package.
   The Android retry built and started the managed client, connected
   over Iroh, and rendered the automatic Personal onboarding prompt.
   The machine-memory reserve guard ended the run during the initial
   reply check. Host `c39bd8610` removes the legacy trajectory tables
   from that check and requires stable final output in the rendered
   conversation. All four focused visibility tests passed.
   Host `e79495fc7` reads the exact completed fixture reply from the
   Android chat document and checks for active typing. All five
   focused output checks passed. Its browser client rendered the
   completed turn, verified separate System and Personal cookies,
   and reconnected after both app and server restarts with zero
   panel-asset pipe misses. The chat client opened in a separate task,
   and the CLI attached to System. The memory guard stopped the run
   during the final client publication check.
   Publication also exposed a compiler dependency conflict between
   the host SDK's own TypeScript and the version its Svelte adapter
   needs at runtime. Host `9b5986236` keeps the SDK's private compiler
   out of the imported package's dependency list. Host `3469e1ede`
   builds the Android smoke's exact local source pins with the
   authored install graph. This avoids attaching the development
   test harness to the workspace that publishes the client. All 14
   compiler-graph, template-selection, and smoke-server tests passed.
   The next Android run paired and reached Personal, then stopped
   when the machine reserve fell below 8 GiB. The last client
   publication and reopen checks remain open. The one-shot
   `vs-verify-cutover-android-final.service` waits for sustained
   headroom and records its result in `profile-rerun/android-publish-when-ready/`.
   The development harness's separate Personal-only browser fixture
   dependency still requires a valid composition for development
   workspaces that run that fixture.
   The fresh-install retry passed the direct CLI provenance and private
   visibility checks, the saved and draft reopen checks after a normal
   restart, and the signed publication race followed by a reviewed
   two-parent merge. Evidence is in
   `p5-native-root-bootstrap-retry-publication-evidence.log`.
   The two fresh installed workspaces passed the shared history test.
   The second workspace used the exact approved seed. Two distinct
   accounts and nodes signed their own group writes, the cross-owner
   write was refused, and the other workspace's protected main
   contained the exact file bytes after reviewed publication. The test
   encodes its SQL with the verified Chronolog package. The result is
   in `p6-native-root-shared-history-after-suites-evidence.log`.
   The P4 benchmark used its sealed package and had recorded 1,360
   successful turns as of November 20, 20:38 UTC, after completing
   the 65-turn soak. At 20:40 UTC the reserve guard stopped the owned
   instance. The benchmark has no completed fork timing, and its turn
   loop starts at one on a fresh launch. See the journal for
   `vs-verify-p4-native-clone-source.service` and the run's memory samples.
   `vs-verify-p4-current-release-fork-benchmark.service` queues a fresh
   run against the prepared release above, after the installed checks and
   ARM64 fallback. It reuses the prepared build, runs the 65-turn soak,
   and gives the model-free 10,000-turn case a 24-hour timeout. The watcher
   exits if the reserve guard stops its instance. The completed result
   and inspection will be recorded under `p4-current-release-fork-benchmark`.
   The subsequent queued benchmark was interrupted to give the requested
   Android smoke the machine. Its partial soak and shutdown evidence is
   retained under `p4-current-release-interrupted-for-android`. The
   benchmark queue waits for the one-shot Android retry to retire.
   Host documentation commit `345b70b63` applies the owner's fresh-start
   and normal-restart guidance directly to the provenance acceptance
   matrix and records how the two local Dolt indexes work. The current
   merged Host candidate includes those changes.
   GitHub still shows no Actions run for fork commit `61862f2`.
   Host `363c3b6cf` requires a verified participant for development
   build inputs, adopted repository reads, and template exchanges.
   Both directions of a reviewed exchange bind the participant's exact
   working copy, and imports register their file bytes to that participant.
   All 30 focused tests passed. See `p5-native-development-access-check.log`.
   Host `0b9d0e64a` additionally saves editor bases and imports checkpoints
   through the verified participant's native history. It binds editor
   reopen and checkpoint to the same participant and records captured
   files as private uploads. Older editor markers require a fresh session.
   All 87 focused Host checks passed across two runs. The product system
   editor driver still reports unavailable, so the editor tests use a
   fixture driver. See `p5-native-editor-participant-check.log` and
   `p5-native-editor-participant-check2.log`.
   Host `e3b039462` makes unauthored runtime builds track protected main.
   A replacement for a private runtime requires the verified owner and
   resolves that owner's current copy. The exact restore action keeps the
   already sealed executable. Runtime preparation refuses an unresolved
   context selector, and all 273 focused tests passed across two runs.
   See `p5-native-runtime-selector-check.log` and
   `p5-native-runtime-selector-check2.log`.
   Host `0380e3a2c` and Base `80fc0db` remove the old context content
   lookup and its workspace RPC. Private build and worker discovery
   inputs enter the engine with the exact revision resolved for the
   verified caller. Protected main remains the default for public
   builds. The client and API documentation generation checks passed.
   The focused Host checks passed 252 cases, with 77 cases rerun after
   the final edits. See `p5-exact-build-state-tests.log`,
   `p5-exact-workspace-source-tests.log`, and
   `p5-exact-workspace-source-final.log`. Host `c0bca1a11` also removes
   the cache of old working heads and retains the retired-context
   fence. All 43 workspace orchestration checks passed again
   (`p5-native-retirement-check.log`). The build suites are queued
   in the release-pair run recorded above.
   Host `83c1d2694` also removes the old runtime fork adapter. Implicit
   panels, context clones, subagents, and development context forks use
   the verified caller's participant revision. A retained runtime fork
   keeps the original selected revision across repeated reservations.
   All 213 focused runtime and workspace orchestration tests passed.
   Four-platform verification and the matching Host pin remain pending
   for the runtime with the verified checkout change.
   The verified runtime commit `61862f2` fast-forwarded
   `panticonic/unified-fork`. GitHub did not create a workflow run
   for that exact push. The three other platform artifacts remain pending.
   The local ARM64 fallback `vs-build-celld-release-linux-arm64.service`
   waits for the installed checks to finish and requires a successful
   toolchain probe. It builds the exact fork commit on Ubuntu 22.04,
   checks the ELF target and runs the result with the Ubuntu ARM64
   libraries under QEMU. See
   `p5-transaction-checkout-linux-arm64-toolchain.log` and
   `p5-transaction-checkout-linux-arm64-build.log`.
   The Windows x64 cross-build `vs-build-celld-release-windows-x64.service`
   uses the same fork commit. Rust 1.94.1 linked a release-profile
   AMD64 Windows executable with the cached Microsoft SDK. The queue
   retains the exe architecture and digest as cross-build evidence.
   Execution on Windows remains a separate release check. See
   `p5-transaction-checkout-windows-x64-toolchain.log`.
   The local merged host `29eead0fa` and Base `404f345` passed
   the host type checks, the authority catalog check, 24 host tests,
   four portable type configurations, and 223 focused template tests.
   Evidence is in `p5-integrated-host-final4.log`,
   `p5-integrated-host-final5.log`, and
   `p5-current-main-base-check10.log`. The final Base check peaked at
   6.6 GiB. A prior scope with a 5 GiB soft limit was killed by
   systemd-oomd at 5.4 GiB. The final check used a 9 GiB hard ceiling
   without that soft limit.
   The companion local merge candidates are Examples `7abe5a667b`,
   Google Workspace `42c5271617`, News `b9d8875b2b`, Personal
   `65737d4227`, System `d6df7f918e`, and System-testing
   `b82cffe650`. Pi's sealed package source is `c6d518177`.
   GitHub run `44550708879` succeeded on its second attempt. The host's
   packaging verifier staged all four exact `9116b07` binaries, validated
   their source, platform, and digest, and ran the Linux x64 executable.
   Host `f5cbd9d82` pins all four. All nine focused host artifact tests
   passed. See `celld-v6-complete-verification.json` and
   `celld-v6-complete-pin-host.log`. The pinned Linux binary also passed the
   signed publication and fresh-history import. See
   `native-gad-d1cc92b-ci-linux.log` and
   `native-gad-seed-protected-ci-linux.log`. The matching companion releases
   are still local candidates.
   The cross-workspace carrier passed on installed Linux with the local
   E19 candidate. The four-platform artifact and pin gate remains.
   - A Chronolog node per workspace node and the `chronolog` host service.
   - Key custody, invite and join, and agent writes through approval (Q4).
   - Chronolog authority for pushes into a shared GAD `main`.
   - **Gate:** two workspace instances on two nodes. Create a group, invite,
     write from both sides, see an owner-row rejection, run a replay flip,
     and push GAD between workspaces authorized by group membership.
4. **Fork: address the remaining limits** (§4.1 item 4).
   The Python scenario passed on `b5742d1`.
5. **Release.** This step goes before handing the result to the owner.
   - Build the fork for linux-arm64, darwin-arm64 and win32-x64.
   - Fill in Vibestudio's `targets.json` digests and wire the `.github`
     workflows.
   - Prepare the Pi packages for the owner to publish, and list the overrides
     to remove.
   - Prepare a merge plan of `feat/dolt-p0` (host) and the template branches
     onto their `main`s for the owner.
     The native installed check uses Host `1db9b4a2c`, Base `23a33cf`,
     and System-testing `d19b0b0`. The first full Host check caught two
     publication timestamp fixtures and one missing website authority field.
     The corrected fixtures passed all 219 focused tests. The full check
     restarts under `vs-verify-native-install-retry.service` after the
     running Linux artifact build and its checkout profile. See
     `p5-native-current-main-release-prepare-first.log` and
     `p5-native-host-release-fixtures.log`.
     Their integrated baselines are Host `integration/gad-participant-main`
     (`676d51652`) and Base `integration/gad-participant-base-main`
     (`79ec699`). The baselines include the signed-receipt retries and
     initial shared history workflow above. The Host
     merge retains one console-history filter and
     pagination implementation and the read-only legacy workspace importer.
     The active Dolt workspace state already contains upstream's slot
     history selection fix. The Base merge preserves the sealed Pi pins
     and upstream messaging and channel origin support. The combined
     checks passed on Base `404f345`. Base `46217b5` adds the upstream
     bindings required by the integrated System-testing candidate
     `b82cffe650`. All 31 focused System-testing tests passed with
     the sealed Pi packages served from an isolated registry. See
     `p5-current-system-testing-check2.log`.
     Use host `feat/gad-participant-admission` and Base
     `feat/gad-participant-cells`, which already include the earlier P0/P2
     changes. A preview against the locally fetched main refs found host
     conflicts in desktop and headless console history, Base conflicts in
     the agent package manifest and pubsub channel, and system-testing
     conflicts in the skill package and test runner. The owner's current
     Chronolog target needs the peer-session port above. Pi can fast-forward
     at the previous preview target. The other template branches
     previewed cleanly. Refresh the previews against the owner's target commits
     before integrating and test the combined changes after resolving conflicts.
     The November 19 previews against the current targets found four host
     conflicts at `e400c4036` (desktop and headless console history, the shared
     console file and the original `WorkspaceDO`), two Base conflicts at
     `c0f4d04` (agent package and pubsub channel), and four System-testing
     conflicts at `5d68e84` (the skill package, two runners and the contract).
     Pi `7016ecdc9` and `c6d518177` merge cleanly. Current Chronolog
     `870fca2` is a fast-forward from `d014879`. The fresh previews of
     Examples `6712d56`, Google Workspace `1ef1b53`, News `83d29bd`,
     Personal `b4cedbd` and System `9323aaf` also merge cleanly.
     The November 20 companion release candidates are Examples
     `d30bd65` (main `a100846`), Google Workspace `9a9c3f2`
     (main `6c5ec09`), News `a8a47b0` (main `b53effc`), Personal
     `f25fa29` (main `51a6774`), and System `c02311e` (main
     `faac2a0`). All five merge trees matched the clean previews. The
     diff against each main contains only the sealed Pi prerelease pins.
     The queued installed check validates those exact companion commits
     before preparing and using the workspace release.
     Refreshed against the locally fetched November 20 main commits. Base
     `3ba4f77` merges `df5525d` and the native participant candidate
     `9168c9d`. Its delivery-history binding accepts omitted message blocks
     and retains the existing text extraction. All 15 focused native history
     tests passed from the merged checkout. See
     `p5-native-context-current-base-main.log`.
     Host `c2b8b3324` merges `2f2a4c25b` and `2fd629be5`. A chained
     extension can resolve its participant's source path before the copy
     is materialized. An explicit materialization request uses that same
     private copy and the requested repository scope. All 142 focused
     filesystem, workspace orchestration and startup tests passed. See
     `p5-native-current-host-merge.log`.
     Base `b18a030` ports main's native observation checks to EvalKernel.
     All 39 journal tests passed. See `p5-native-context-current-base-journal.log`.
     System-testing `571618b` merges `f657e13` and `5296454`. Ordinary
     suites, retained triage, and native evidence probes all use the
     dynamic-code execution API. All 32 focused runner, open-goal, and
     compaction tests passed with the current Host schemas and merged
     Base. See `p5-native-current-system-merge-schemas.log`. Packaging and
     the installed fresh/reopen/publication check remain queued for this
     exact set of commits.
     Pi `c6d518177` is a fast-forward from `2f530d750`. The five staged
     archives passed fresh SHA-256, npm SHA-512, package identity, contents,
     and dependency checks. See `pi-native-release-package-owner-verification.log`.
     Current Pi main `7016ecdc9` adds a Nix model catalog pin and previews
     as a clean merge with that package source.
     Publish the five Pi packages in dependency order, then remove the local
     overrides and verify an install from the registry.
   - Host `34006ed5d` removes the retired SQLite `WorkspaceDO`, the
     import driver, and the import-only RPCs from the Dolt state class.
     The deployment catalog now lists only `WorkspaceStateDO` for workspace
     state. Existing SQLite files remain on disk. The merge also restores
     the full slot-history record required by the public schema. All 244
     focused tests passed, including two real E19 Dolt workspace probes.
     The generated catalog and authority checks passed. See
     `p5-native-remove-workspace-state-legacy-final.log`,
     `p5-native-workspace-state-deployment.log` and
     `p5-native-host-catalog-final.log`.
6. **Chronolog cleanup** (§4.4 items 1–6). Can be done before P6, which
   uses it.
7. **The pre-existing failures**, if the owner wants them fixed. They were
   present before this work and are not regressions:
   - host: `src/cli/agent/index.test.ts`;
   - Base: `vcs-skill-release-generator`, a stale content manifest;
   - system-testing: 4 failures in `cli.test.ts`;
   - Pi: 51 failures on its `main`;
   - lane: `state-args-immediate-snapshot`, whose model script has a guest type
     error. `create-panel`, `workspace-test-runner-extension`, `call-do-method`
     and `eval-exact-authority` had concrete host/validator defects repaired in
     the Nov 17 continuation. Steering belongs to the mandatory P4 investigation
     in item 1, not this optional baseline list;
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

Expected: R2 etag XFAIL (E2); `pi` and `opencode` skipped. Fork changes
go on a `fix/*` or `feature/*`
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
