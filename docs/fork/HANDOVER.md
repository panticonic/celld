# Handover: the unified celld fork, Vibestudio and Chronolog (2026-11-15)

This is the entry point for an agent that takes over the whole project. It
says what the project is and what the owner decided, where every piece of
work lives, what is done and how it was verified, and what is left, in order.
The detailed per-stream handoffs it points to are authoritative for their own
details; this file is authoritative for the overall order and the rules.

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
The 10,000-turn fork latency remains outstanding. The isolated P5 worktrees
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
   it. P6 still needs this carrier wired to Vibestudio's workspace
   instances and Chronolog membership.
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
| P5 GAD decentralized        | Local participant storage, replay, file routing, native integration, protected publication and retirement passed focused tests. Disposable managed instances verified separate editable folders, paired publication and normal shutdown. Default startup cutover, the remaining public history methods, and acceptance benchmarks are open. |
| P6 Chronolog capability     | The fork's direct transfer and Chronolog's two-node replay are verified. The Vibestudio service and membership check for publication are open. |

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

### 4.4 Chronolog on the fork (`feature/celld-host`)

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
   4. Run a model-free soak of an agent cell past 64 turns, so compaction
      runs in the product. Verify `WorkspaceStateDO`'s inherited common
      compaction schedule on the real class.
   5. The agent-cell restart probe passes through activation-loss replay.
      Measure fork latency at 10,000 turns.
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
   Needs §4.1 item 3, the cross-workspace P2P carrier, and Chronolog step 6
   below.
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
