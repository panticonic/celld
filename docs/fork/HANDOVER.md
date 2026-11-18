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

Workspace startup still needs a complete cutover to the participant
source and the remaining public semantic operations. Incremental peer
prefixes, remaining structural choices and
interactive approval still need their cutover checks. The native
fixture uses synthetic approval receipts.
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
| P5 GAD decentralized        | Participant storage, exact-commit approvals, published context integration, retirement archival, and offline replay are implemented locally. A fresh disposable workspace with 5,135 files replayed and forked successfully. Live workspace routing, the host approval connection, and physical participant retirement remain open. |
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
