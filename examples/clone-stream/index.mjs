// `clone()` of a response or request whose body is a stream. workerd tees the
// body: the original and the clone each read all of it, in either order.
function stream(parts, delayMs = 0) {
  return new ReadableStream({
    async start(controller) {
      for (const part of parts) {
        if (delayMs) await new Promise((resolve) => setTimeout(resolve, delayMs));
        controller.enqueue(new TextEncoder().encode(part));
      }
      controller.close();
    },
  });
}

export default {
  async fetch(request) {
    const url = new URL(request.url);
    switch (url.pathname) {
      // A response built from a stream: read the clone first, then the original.
      case "/response": {
        const original = new Response(stream(["a", "b", "c"], 5), {
          status: 201,
          headers: { "x-kind": "streamed" },
        });
        const clone = original.clone();
        const fromClone = await clone.text();
        const fromOriginal = await original.text();
        return Response.json({
          fromClone,
          fromOriginal,
          status: clone.status,
          header: clone.headers.get("x-kind"),
        });
      }
      // The other order, and a clone of a clone.
      case "/response-twice": {
        const original = new Response(stream(["1", "2", "3"]));
        const first = original.clone();
        const second = first.clone();
        return Response.json({
          original: await original.text(),
          second: await second.text(),
          first: await first.text(),
        });
      }
      // A clone the original's reader outruns by a long way: the tee buffers.
      case "/response-large": {
        const chunk = "x".repeat(64 * 1024);
        const original = new Response(stream(new Array(32).fill(chunk)));
        const clone = original.clone();
        const a = (await original.arrayBuffer()).byteLength;
        const b = (await clone.arrayBuffer()).byteLength;
        return Response.json({ original: a, clone: b });
      }
      // The response of a fetch() is a host stream; it tees as well.
      case "/fetched": {
        const upstream = await fetch(new URL("/response", request.url));
        const clone = upstream.clone();
        const [a, b] = await Promise.all([upstream.json(), clone.json()]);
        return Response.json({ same: JSON.stringify(a) === JSON.stringify(b), fromClone: b.fromClone });
      }
      // A body that was already read cannot be cloned.
      case "/used": {
        const response = new Response(stream(["x"]));
        await response.text();
        let error = null;
        try {
          response.clone();
        } catch (e) {
          error = e.constructor.name;
        }
        return Response.json({ error });
      }
      // A request with a streaming body, which already worked.
      case "/request": {
        const original = new Request("https://example.com/", { method: "POST", body: stream(["p", "q"]) , duplex: "half" });
        const clone = original.clone();
        return Response.json({ clone: await clone.text(), original: await original.text() });
      }
    }
    return new Response("not found", { status: 404 });
  },
};
