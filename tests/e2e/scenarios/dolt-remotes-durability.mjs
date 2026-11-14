// dolt-remotes-durability: the operations of Dolt remotes (grant, remote add,
// fetch, push and its accepted-push notification, pull with a resolver, revoke,
// remote remove) are durable once answered: after each, the node is SIGKILLed
// and its local runtime state deleted, so the next start restores the cells
// from the replication log alone. See dolt-durability.mjs for why.
import { run } from "../lib/dsl.mjs";
import { eq, survives } from "./dolt-durability.mjs";

const call = async (t, route, options) => {
  const res = await t.fetch(route, options);
  if (res.status !== 200) throw new Error(`${route} answered ${res.status}: ${res.text.slice(0, 300)}`);
  return res.json;
};
const state = (t, g) => call(t, `/state?g=${g}`);
const headOf = async (t, g, ref = "HEAD") => (await call(t, `/hash?g=${g}&ref=${ref}`)).hash;
const bodies = (notes) => notes.map((note) => note.body);

const steps = [
  run("a shared graph with a commit, and an agent forked from it", async (t) => {
    await call(t, "/set?g=gad&id=1&body=one");
    t.vars.base = (await call(t, "/commit?g=gad&message=base")).hash;
    await call(t, "/fork?from=gad&to=agent");
  }),
  ...survives(
    "grant and remote add (the token still works after the restore)",
    async (t) => {
      const linked = await call(t, "/link?source=gad&holder=agent&label=agent&push=main,work/*&branches=*");
      t.vars.grant = linked.grant;
      eq(t, (await state(t, "gad")).grants, ["agent"], "grants");
    },
    async (t) => {
      eq(t, (await state(t, "gad")).grants, ["agent"], "the grant");
      eq(t, (await state(t, "agent")).remotes.map((remote) => remote.name), ["origin"], "the remote");
      // The token the holder stored is the one the source's hash matches.
      const fetched = await call(t, "/fetch?g=agent&branches=main");
      t.assert(fetched.branches.main.length === 40, JSON.stringify(fetched));
    },
  ),
  ...survives(
    "fetch (the tracking branches)",
    async (t) => {
      await call(t, "/set?g=gad&id=2&body=two");
      t.vars.gadHead = (await call(t, "/commit?g=gad&message=two")).hash;
      const fetched = await call(t, "/fetch?g=agent");
      eq(t, fetched.branches.main, t.vars.gadHead, "the fetched tip");
    },
    async (t) => {
      const { tracking } = await state(t, "agent");
      t.assert(tracking.includes(`origin/main=${t.vars.gadHead.slice(0, 8)}`), JSON.stringify(tracking));
    },
  ),
  ...survives(
    "pull with a resolver (a conflict resolved in the merge's transaction)",
    async (t) => {
      await call(t, "/set?g=gad&id=1&body=gad-one");
      t.vars.gadHead = (await call(t, "/commit?g=gad&message=gad-one")).hash;
      await call(t, "/set?g=agent&id=1&body=agent-one");
      await call(t, "/commit?g=agent&message=agent-one");
      const pulled = await call(t, "/pull?g=agent&branch=main&message=merged&resolve=text");
      t.assert(pulled.conflicts.length === 0 && pulled.resolved.conflicts === 1, JSON.stringify(pulled));
      t.vars.merged = await headOf(t, "agent");
    },
    async (t) => {
      eq(t, await headOf(t, "agent"), t.vars.merged, "head");
      const now = await state(t, "agent");
      eq(t, bodies(now.notes), ["agent-one + gad-one", "two"], "rows");
      eq(t, now.log[0], "merged", "log");
    },
  ),
  ...survives(
    "push, and the target's notification (ondoltpush)",
    async (t) => {
      const pushed = await call(t, "/push?g=agent&branch=main");
      eq(t, pushed.hash, t.vars.merged, "the pushed tip");
      eq(t, pushed.unchanged, false, "unchanged");
    },
    async (t) => {
      const gad = await state(t, "gad");
      eq(t, gad.branches.main, t.vars.merged, "the target's branch");
      eq(t, gad.head, t.vars.merged, "the target's head");
      eq(t, bodies(gad.notes), ["agent-one + gad-one", "two"], "the target's rows");
      const pushes = await call(t, "/pushes?g=gad");
      eq(t, pushes.map((push) => [push.branch, push.from, push.to]),
        [["main", t.vars.gadHead, t.vars.merged]], "the notifications");
    },
  ),
  ...survives(
    "push that creates a branch",
    async (t) => {
      await call(t, "/branch?g=agent&name=work/a");
      await call(t, "/checkout?g=agent&name=work/a");
      await call(t, "/set?g=agent&id=5&body=on-work");
      t.vars.work = (await call(t, "/commit?g=agent&message=work")).hash;
      const pushed = await call(t, "/push?g=agent&branch=work/a");
      eq(t, pushed.created, true, "created");
    },
    async (t) => {
      eq(t, (await state(t, "gad")).branches["work/a"], t.vars.work, "the new branch");
      const pushes = await call(t, "/pushes?g=gad");
      eq(t, pushes.length, 2, "notifications");
      eq(t, pushes[1].created, true, "the second says it created the branch");
    },
  ),
  ...survives(
    "revoke (the token stays refused after the restore)",
    async (t) => { await call(t, `/revoke?g=gad&id=${t.vars.grant}`); },
    async (t) => {
      eq(t, (await state(t, "gad")).grants, [], "grants");
      eq(t, (await call(t, "/fetch?g=agent")).code, "ERR_DOLT_REMOTE_FORBIDDEN", "a fetch with the revoked token");
    },
  ),
  ...survives(
    "remote remove",
    async (t) => { await call(t, "/remote-remove?g=agent&name=origin"); },
    async (t) => {
      const now = await state(t, "agent");
      eq(t, now.remotes, [], "remotes");
      eq(t, now.tracking, [], "tracking branches");
    },
  ),
];

export default {
  example: "dolt-remotes",
  id: "dolt-remotes-durability",
  title: "every acknowledged Dolt remote operation (grant, fetch, pull, push, accepted-push notification, revoke) survives a SIGKILL and a restore",
  steps,
};
