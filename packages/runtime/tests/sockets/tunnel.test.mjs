import assert from "node:assert/strict";
import test from "node:test";
import { setImmediate } from "node:timers/promises";
import { importRuntime } from "../compiled-runtime.mjs";

const { tunnelSockets } = await importRuntime("sockets/tunnel.ts");

function socket({
  chunks = [],
  keepOpen = false,
  write = () => {},
  finish = () => {},
  closeError,
} = {}) {
  const closed = Promise.withResolvers();
  const writes = [];
  let readableControl;
  let writableControl;
  let closes = 0;
  return {
    readable: new ReadableStream({
      start(controller) {
        readableControl = controller;
        for (const chunk of chunks) controller.enqueue(chunk);
        if (!keepOpen) controller.close();
      },
    }),
    writable: new WritableStream({
      start(controller) {
        writableControl = controller;
      },
      async write(chunk) {
        await write(chunk);
        writes.push(chunk);
      },
      close: finish,
    }),
    closed: closed.promise,
    disconnect: closed.resolve,
    send(chunk) {
      readableControl.enqueue(chunk);
    },
    end() {
      readableControl.close();
    },
    writes,
    get closes() {
      return closes;
    },
    async close() {
      closes++;
      readableControl.error(new Error("closed"));
      writableControl.error(new Error("closed"));
      closed.resolve();
      if (closeError) throw closeError;
    },
  };
}

test("a fulfilled Socket.closed does not discard a pending final write", async () => {
  const entered = Promise.withResolvers();
  const flushed = Promise.withResolvers();
  const reply = new Uint8Array([4, 5, 6]);
  const left = socket({
    write: async () => {
      entered.resolve();
      await flushed.promise;
    },
  });
  const right = socket({ chunks: [reply] });
  const drained = tunnelSockets(left, right);
  // Attach the observer before triggering disconnect to keep the failing implementation's
  // rejection in this test rather than Node's unhandled-rejection diagnostics.
  const observed = drained.then(
    () => undefined,
    (error) => error,
  );
  await entered.promise;
  right.disconnect();
  await setImmediate();
  try {
    assert.equal(left.closes, 0);
    assert.equal(right.closes, 0);
  } finally {
    flushed.resolve();
    await observed;
  }
  await drained;
  assert.deepEqual(left.writes, [reply]);
});

test("read EOF in one direction leaves the reverse direction available", async () => {
  const readEnd = Promise.withResolvers();
  const marker = new Uint8Array([7]);
  const late = new Uint8Array([8, 9]);
  const left = socket({ keepOpen: true, finish: readEnd.resolve });
  const right = socket({ chunks: [marker] });
  const drained = tunnelSockets(left, right);
  await readEnd.promise;
  assert.deepEqual(left.writes, [marker]);
  assert.equal(left.closes, 0);
  assert.equal(right.closes, 0);
  left.send(late);
  left.end();
  await drained;
  assert.deepEqual(right.writes, [late]);
});

test("cleanup attempts both sockets and hides a rejected close reason", async () => {
  const left = socket({
    keepOpen: true,
    closeError: Error("private close details"),
    write() {
      throw Error("private transport details");
    },
  });
  const right = socket({
    chunks: [new Uint8Array([1])],
  });
  // The right-to-left pipe must fail; the opposite pipe remains open until cleanup.
  await assert.rejects(tunnelSockets(left, right), (error) => {
    assert.equal(error.message, "SOCKET_TUNNEL_FAILED");
    assert.equal(error.stack, "Error: SOCKET_TUNNEL_FAILED");
    return true;
  });
  assert.equal(left.closes, 1);
  assert.equal(right.closes, 1);
});

test("a failed direction closes both sockets without waiting for the other direction", async () => {
  const failed = Promise.withResolvers();
  const left = socket({
    keepOpen: true,
    write() {
      failed.resolve();
      throw Error("raw upstream secret");
    },
  });
  const right = socket({ chunks: [new Uint8Array([1])] });
  const drained = tunnelSockets(left, right);
  const observed = drained.then(
    () => undefined,
    (error) => error,
  );
  await failed.promise;
  await setImmediate();
  try {
    assert.equal(left.closes, 1);
    assert.equal(right.closes, 1);
  } finally {
    // Release test streams even when the regression fails before production cleanup.
    if (left.closes === 0) await left.close();
    if (right.closes === 0) await right.close();
  }
  const error = await observed;
  assert.equal(error.message, "SOCKET_TUNNEL_FAILED");
  assert.equal(error.stableCode, "SOCKET_TUNNEL_FAILED");
  assert.equal(error.stack, "Error: SOCKET_TUNNEL_FAILED");
});
