const main = `
  import path from "node:path";
  import { describePath } from "./sibling.js";
  export default {
    fetch() {
      return Response.json(describePath(path.join("/workspace", "sample.txt")));
    },
  };
`;

const sibling = `
  import { basename, isAbsolute } from "node:path";
  export function describePath(filename) {
    return { filename: basename(filename), absolute: isAbsolute(filename) };
  }
`;

export default {
  fetch(request, env) {
    const worker = env.LOADER.get("external-imports", () => ({
      compatibilityDate: "2025-01-01",
      mainModule: "main.js",
      modules: { "main.js": main, "sibling.js": sibling },
    }));
    return worker.getEntrypoint().fetch(request);
  },
};
