// The `p2p` binding: `env.P2P.connect()` and the stream an inbound peer opens
// into a Durable Object's `onp2pstream()`.
//
// A stream is a pair of Web streams over one QUIC bidirectional stream, with
// the same shape `cloudflare:sockets` gives a TCP socket (`readable`,
// `writable`, `closed`, `close()`) plus the peer's authenticated node id.
// Reads and writes go through `__p2p_*` host ops.
//
// Who owns a stream depends on who opened it. A Durable Object's stream (a
// dial from any of its events, or a stream a peer opened into its
// `onp2pstream`) belongs to the cell: the host adopts it as an outbound
// transport of the cell, it pins the cell like an outbound WebSocket, and it
// lives until it is closed, both directions finish, or the cell leaves the
// node. A Worker's stream belongs to its request and the host closes it when
// the request retires.
(function () {
  const HEX_ID = /^[0-9a-f]{64}$/;

  // QUIC reset codes celld itself uses; applications should pick codes >= 256.
  const CODES = Object.freeze({
    NO_HANDLER: 1,
    NOT_OWNER: 2,
    HANDLER_FAILED: 3,
    BUSY: 4,
    CELL_ENDED: 5,
  });

  const toBytes = (chunk) =>
    chunk instanceof Uint8Array
      ? chunk
      : ArrayBuffer.isView(chunk)
        ? new Uint8Array(chunk.buffer, chunk.byteOffset, chunk.byteLength)
        : chunk instanceof ArrayBuffer
          ? new Uint8Array(chunk)
          : (() => {
            throw new TypeError(
              "A p2p stream accepts ArrayBuffer and ArrayBufferView chunks.");
          })();

  class P2PStream {
    constructor(id, remoteNodeId, alpn) {
      this._id = id;
      this._closing = false;
      // Each direction is done when its end-of-stream has been seen (reads)
      // or sent (writes); the stream is over when both are.
      this._readDone = false;
      this._writeDone = false;
      this.remoteNodeId = remoteNodeId;
      this.alpn = alpn;
      let resolveClosed;
      let rejectClosed;
      this._closed = new Promise((resolve, reject) => {
        resolveClosed = resolve;
        rejectClosed = reject;
      });
      // A reset rejects `closed`; a caller that never looks at it must not
      // be charged with an unhandled rejection.
      this._closed.catch(() => {});
      this._resolveClosed = resolveClosed;
      this._rejectClosed = rejectClosed;
      const stream = this;
      // highWaterMark 0: never read ahead of the consumer, so a stream that
      // is not being read holds nothing and applies backpressure to the peer.
      this._readable = new ReadableStream({
        async pull(controller) {
          if (stream._closing) {
            controller.close();
            return;
          }
          let bytes;
          try {
            bytes = await __p2p_read(stream._id);
          } catch (error) {
            stream._end(error);
            throw error;
          }
          if (stream._closing) {
            controller.close();
            return;
          }
          if (bytes.byteLength === 0) {
            // The peer finished its side, cleanly.
            controller.close();
            stream._readDone = true;
            stream._endIfDone();
            return;
          }
          controller.enqueue(bytes);
        },
        cancel() {
          // The consumer wants no more. The peer's remaining bytes are
          // refused when the stream closes.
          stream._readDone = true;
          stream._endIfDone();
        },
      }, { highWaterMark: 0 });
      this._writable = new WritableStream({
        async write(chunk) {
          if (stream._closing) throw new TypeError("This p2p stream is closed.");
          try {
            await __p2p_write(stream._id, toBytes(chunk));
          } catch (error) {
            // A peer that reset the stream, or a lost connection, ends it.
            stream._end(error);
            throw error;
          }
        },
        async close() {
          if (stream._closing) return;
          await __p2p_shutdown(stream._id);
          stream._writeDone = true;
          stream._endIfDone();
        },
        abort() {
          stream.close(0);
        },
      });
    }

    get readable() { return this._readable; }
    get writable() { return this._writable; }
    get closed() { return this._closed; }

    // The stream is over. Resolve `closed` when it ended cleanly (both
    // directions finished) and reject it with the error when the peer reset
    // it or the connection was lost, then give the host its slot back.
    _end(error) {
      if (this._closing) return;
      this._closing = true;
      if (error === undefined) this._resolveClosed(undefined);
      else this._rejectClosed(error);
      __p2p_close(this._id, -1);
    }

    _endIfDone() {
      if (this._readDone && this._writeDone) this._end(undefined);
    }

    // Close both directions. Without a code the close is orderly: what was
    // written is delivered, then the peer reads end-of-stream. With a code the
    // stream is reset, and the peer's reads and writes fail with it. `closed`
    // resolves: a close the caller asked for is not an error. The returned
    // promise resolves once the stream is over, however it ended.
    close(code) {
      if (!this._closing) {
        this._closing = true;
        this._resolveClosed(undefined);
        __p2p_close(this._id, code === undefined ? -1 : Number(code));
      }
      // Always resolves, even when the stream had already ended in a reset:
      // the outcome is `closed`'s to report, and closing is idempotent.
      return this._closed.then(() => {}, () => {});
    }
  }

  const parseOptions = (options) => {
    const addrs = options?.addrs ?? [];
    if (!Array.isArray(addrs)) {
      throw new TypeError("connect(): options.addrs must be an array of \"ip:port\" strings.");
    }
    const relay = options?.relay;
    if (relay !== undefined && typeof relay !== "string") {
      throw new TypeError("connect(): options.relay must be a relay URL string.");
    }
    return { addrs: addrs.map(String), relay };
  };

  // One binding: the ALPN is fixed by configuration, never by the caller.
  const makeP2P = ({ binding, alpn }) => {
    const p2p = {
      get nodeId() {
        const id = __p2p_node_id();
        if (!id) throw new Error("The p2p endpoint is not running on this node.");
        return id;
      },
      get alpn() { return alpn; },
      async connect(nodeId, options = undefined) {
        if (typeof nodeId !== "string" || !HEX_ID.test(nodeId)) {
          throw new TypeError(
            "connect(): nodeId must be a 64-character lowercase hex public key.");
        }
        const { addrs, relay } = parseOptions(options);
        // The Durable Object the caller runs in, if any: its stream belongs to
        // that cell. Empty for a Worker.
        const scope = __celld.__actorScope?.() ?? "";
        const raw = await __p2p_connect(
          JSON.stringify({ alpn, nodeId, addrs, relay, scope }));
        const info = JSON.parse(raw);
        return new P2PStream(info.id, info.remoteNodeId, alpn);
      },
    };
    Object.defineProperty(p2p, Symbol.toStringTag, { value: "P2P:" + binding });
    return p2p;
  };

  // Called by `__dispatchTo` for every fetch into a Durable Object. An inbound
  // stream arrives as an internal request whose URL carries a one-time claim
  // token; anything else, including a request that only imitates the URL,
  // returns undefined and goes to the application's `fetch` as usual.
  const PREFIX = "https://p2p.celld.invalid/__celld/p2p/accept/";
  const accept = (inst, url, scope, run) => {
    if (typeof url !== "string" || !url.startsWith(PREFIX)) return undefined;
    const claimed = __p2p_claim(url.slice(PREFIX.length), scope);
    if (!claimed) return undefined;
    const info = JSON.parse(claimed);
    const stream = new P2PStream(info.id, info.remoteNodeId, info.alpn);
    stream.binding = info.binding;
    return (async () => {
      if (typeof inst.onp2pstream !== "function") {
        stream.close(CODES.NO_HANDLER);
        return new Response("this Durable Object has no onp2pstream()", { status: 501 });
      }
      // The stream is the cell's from here on, so the core has to agree to
      // hold the cell for it; at the budget the stream is reset and the
      // handler never sees it.
      try {
        await __p2p_adopt(info.id, scope);
      } catch (error) {
        stream._closing = true;
        stream._resolveClosed(undefined);
        return new Response(String(error?.message ?? error), { status: 503 });
      }
      try {
        await run(() => inst.onp2pstream(stream));
      } catch (error) {
        stream.close(CODES.HANDLER_FAILED);
        throw error;
      }
      // The stream outlives this event: the handler may have handed it to
      // code that keeps using it, and it ends when it is closed, both
      // directions finish, or the cell leaves.
      return new Response(null, { status: 200 });
    })();
  };

  __celld.__makeP2P = makeP2P;
  __celld.__p2pAccept = accept;
  __celld.__p2pCodes = CODES;
})();
