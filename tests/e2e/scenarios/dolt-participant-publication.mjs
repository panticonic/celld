import { run } from "../lib/dsl.mjs";
import { eq } from "./dolt-durability.mjs";

async function call(t, route) {
  const response = await t.fetch(route);
  t.assert(response.status === 200, `${route.split("?")[0]} returned ${response.status}`);
  return response.json;
}

export default {
  example: "dolt-remotes",
  id: "dolt-participant-publication",
  title: "concurrent participant pushes converge after one conflict resolution",
  steps: [
    run("publish independent work against the same shared tip", async (t) => {
      await call(t, "/set?g=gad&id=1&body=shared");
      const base = (await call(t, "/commit?g=gad&message=initial")).hash;
      const savedHeads = new Map();
      for (const [participant, row] of [["alice", 2], ["bob", 3]]) {
        await call(t, `/fork?from=gad&to=${participant}`);
        await call(t, `/set?g=${participant}&id=1&body=${participant}`);
        await call(t, `/set?g=${participant}&id=${row}&body=${participant}-only`);
        const saved = await call(t, `/commit?g=${participant}&message=${participant}-work`);
        savedHeads.set(participant, saved.hash);
        await call(t, `/link?source=gad&holder=${participant}&label=${participant}&branches=main`);
        const forbidden = await call(t, `/push?g=${participant}&branch=main`);
        eq(t, forbidden.code, "ERR_DOLT_REMOTE_FORBIDDEN", `${participant} cannot publish with a read grant`);
      }
      eq(t, (await call(t, "/state?g=gad")).notes, [{ id: 1, body: "shared" }], "shared state before publication");
      const push = (participant) => call(t, `/push?g=${participant}&remote=approved&branch=main`);
      const grantIds = await Promise.all(["alice", "bob"].map(async (participant) => {
        const grant = await call(t, `/link?source=gad&holder=${participant}&name=approved&label=approved-${participant}&fetch=0&push=main&branches=main`);
        t.assert(typeof grant.grant === "string" && !grant.remote?.error, `could not link ${participant}'s publication grant`);
        return grant.grant;
      }));
      const firstPushes = await Promise.all([push("alice"), push("bob")]);
      const accepted = firstPushes.filter((result) => Boolean(result.hash));
      const refused = firstPushes.filter((result) => result.code === "ERR_DOLT_NON_FAST_FORWARD");
      eq(t, [accepted.length, refused.length], [1, 1],
        `one push wins and one has a stale basis (${JSON.stringify(firstPushes)})`);
      const winner = accepted[0].hash === savedHeads.get("alice") ? "alice" : "bob";
      const loser = winner === "alice" ? "bob" : "alice";
      const winnerHead = savedHeads.get(winner);
      eq(t, accepted[0].hash, winnerHead, "the exact winning revision");
      eq(t, (await call(t, "/state?g=gad")).head, winnerHead, "shared tip after the race");

      const merged = await call(t, `/pull?g=${loser}&branch=main&message=integrate&resolve=text`);
      eq(t, [merged.fastForward, merged.conflicts, merged.resolved.conflicts], [false, [], 1], "one conflict resolved");
      const published = await push(loser);
      eq(t, published.hash, merged.hash, "publish the exact merged revision");
      eq(t, (await push(loser)).unchanged, true, "retry is idempotent");
      const shared = await call(t, "/state?g=gad");
      eq(t, shared.head, published.hash, "the shared checkout has the exact published revision");
      eq(t, shared.notes, [
        { id: 1, body: "alice + bob" },
        { id: 2, body: "alice-only" },
        { id: 3, body: "bob-only" },
      ], "both clean changes and the resolved change survive");
      eq(t, shared.log.includes("alice-work") && shared.log.includes("bob-work"), true, "both original revisions are reachable");
      const notifications = await call(t, "/pushes?g=gad");
      eq(t, notifications.map(({ branch, from, to }) => [branch, from, to]), [
        ["main", base, winnerHead],
        ["main", winnerHead, published.hash],
      ], "shared history advances along one chain");
      const followed = await call(t, `/pull?g=${winner}&branch=main`);
      eq(t, followed.fastForward, true, "the winning copy fast-forwards to the integrated result");
      eq(t, (await call(t, `/state?g=${winner}`)).head, shared.head, "both copies converge");
      for (const id of grantIds) await call(t, `/revoke?g=gad&id=${id}`);
    }),
  ],
};
