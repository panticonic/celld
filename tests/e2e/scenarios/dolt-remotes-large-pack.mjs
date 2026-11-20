import { run } from "../lib/dsl.mjs";
import { eq } from "./dolt-durability.mjs";

export default {
  example: "dolt-remotes",
  id: "dolt-remotes-large-pack",
  title: "fetches and pushes packs larger than the SQL value limit",
  steps: [
    run("transfer commits containing multiple megabytes of small values", async (t) => {
      const call = async (route) => {
        const response = await t.fetch(route);
        t.assert(response.status === 200 && !response.json?.error, `${route}: ${response.text.slice(0, 300)}`);
        return response.json;
      };
      await call("/set?g=gad&id=1&body=base");
      await call("/commit?g=gad");
      await call("/fork?from=gad&to=agent");
      await call("/link?source=gad&holder=agent&push=main");

      const first = await call("/add-payload-batch?g=gad");
      const fetched = await call("/fetch?g=agent");
      eq(t, fetched.branches.main, first.hash, "fetched revision");
      t.assert(fetched.bytes > 2_200_000, "fetch exceeded the SQL value limit");
      await call("/pull?g=agent&branch=main");
      eq(t, (await call("/payload-size?g=agent")).bytes, 48 * 65536, "pulled payload size");

      const second = await call("/add-payload-batch?g=agent");
      const pushed = await call("/push?g=agent&branch=main");
      eq(t, pushed.hash, second.hash, "pushed revision");
      eq(t, (await call("/payload-size?g=gad")).bytes, 96 * 65536, "published payload size");
    }),
  ],
};
