// The `p2p` binding: `env.P2P.connect()` and the stream an inbound peer opens
// into a Durable Object's `onp2pstream()`.
//
// A stream is a pair of Web streams over one QUIC bidirectional stream, with
// the same shape `cloudflare:sockets` gives a TCP socket (`readable`,
// `writable`, `closed`, `close()`) plus the peer's authenticated node id.
// Reads and writes go through `__p2p_*` host ops. The event that holds the
// stream owns it: the host closes it when the event retires.
(function () {
  const HEX_ID = /^[0-9a-f]{64}$/;

  // QUIC reset codes celld itself uses; applications should pick codes >= 256.
  const CODES = Object.freeze({
    NO_HANDLER: 1,
    NOT_OWNER: 2,
    HANDLER_FAILED: 3,
    BUSY: 4,
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
      this.remoteNodeId = remoteNodeId;
      this.alpn = alpn;
      let resolveClosed;
      this._closed = new Promise((resolve) => { resolveClosed = resolve; });
      this._resolveClosed = resolveClosed;
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
            stream._finish();
            throw error;
          }
          if (stream._closing || bytes.byteLength === 0) {
            controller.close();
            return;
          }
          controller.enqueue(bytes);
        },
        cancel() {},
      }, { highWaterMark: 0 });
      this._writable = new WritableStream({
        async write(chunk) {
          if (stream._closing) throw new TypeError("This p2p stream is closed.");
          await __p2p_write(stream._id, toBytes(chunk));
        },
        async close() {
          if (!stream._closing) await __p2p_shutdown(stream._id);
        },
        abort() {
          stream.close(0);
        },
      });
    }

    get readable() { return this._readable; }
    get writable() { return this._writable; }
    get closed() { return this._closed; }

    _finish() {
      if (this._closing) return;
      this._closing = true;
      this._resolveClosed(undefined);
    }

    // Close both directions. Without a code the close is orderly: what was
    // written is delivered, then the peer reads end-of-stream. With a code the
    // stream is reset, and the peer's reads and writes fail with it.
    close(code) {
      if (this._closing) return this._closed;
      this._finish();
      __p2p_close(this._id, code === undefined ? -1 : Number(code));
      return this._closed;
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
        const raw = await __p2p_connect(JSON.stringify({ alpn, nodeId, addrs, relay }));
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
      try {
        await run(() => inst.onp2pstream(stream));
      } catch (error) {
        stream.close(CODES.HANDLER_FAILED);
        throw error;
      }
      // The event lasts exactly as long as the handler (and its waitUntil
      // work); whatever the handler left open ends with it.
      stream.close();
      return new Response(null, { status: 200 });
    })();
  };

  __celld.__makeP2P = makeP2P;
  __celld.__p2pAccept = accept;
  __celld.__p2pCodes = CODES;
})();
