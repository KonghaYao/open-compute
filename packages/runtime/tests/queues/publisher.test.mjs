import assert from "node:assert/strict";
import test from "node:test";
import { deserialize, serialize } from "node:v8";
import { compileRuntime, moduleUrl } from "../compiled-runtime.mjs";
import { frameMessages } from "./frame.mjs";

const asyncHooks = moduleUrl(`
  export class AsyncLocalStorage {
    constructor() { this.stack = []; }
    run(store, fn) { this.stack.push(store); try { return fn(); } finally { this.stack.pop(); } }
    getStore() { return this.stack.at(-1); }
  }
`);
const outputGateUrl = moduleUrl(
  await compileRuntime("durable-objects/output-gate.ts", {
    "node:async_hooks": asyncHooks,
  }),
);
const { DoOutputGate, runWithOutputGate, FLUSH_OUTPUT, FINALIZE_OUTPUT } =
  await import(outputGateUrl);
const adapterUrl = moduleUrl(
  await compileRuntime("queues/native-adapter.ts", {
    "../bindings/json-body.js": moduleUrl(
      await compileRuntime("bindings/json-body.ts"),
    ),
    "./frame.js": moduleUrl(await compileRuntime("queues/frame.ts")),
    "./metrics.js": moduleUrl(await compileRuntime("queues/metrics.ts")),
  }),
);
const { QueuePublisher } = await import(
  moduleUrl(
    await compileRuntime("queues/publisher.ts", {
      "./frame.js": moduleUrl(await compileRuntime("queues/frame.ts")),
      "./metrics.js": moduleUrl(await compileRuntime("queues/metrics.ts")),
      "./native-adapter.js": adapterUrl,
      "../durable-objects/output-gate.js": outputGateUrl,
    }),
  )
);

const codec = { decodeV8: deserialize };
const bytes = (text) => new TextEncoder().encode(text);
const batch = (messages) =>
  JSON.stringify({
    messages: messages.map(([body, contentType, delaySecs]) => ({
      body: Buffer.from(body).toString("base64"),
      contentType,
      delaySecs,
    })),
  });

function transport(handler) {
  return {
    send(frame, operationId) {
      return handler("send", frame, operationId);
    },
    sendBatch(frame, operationId) {
      return handler("batch", frame, operationId);
    },
    async finalize(operationId) {
      await handler("finalize", operationId);
    },
    metrics() {
      return handler("metrics");
    },
  };
}

function gateStorage() {
  const rows = [];
  return {
    sql: {
      exec(query, ...params) {
        const text = String(query);
        if (text.includes("CREATE TABLE"))
          return {
            one() {
              return {};
            },
            toArray() {
              return [];
            },
          };
        if (text.includes("INSERT")) {
          const row = {
            id: rows.length + 1,
            kind: params[0],
            publisher: params[1],
            payload: params[2],
            operation_id: params[3],
            state: "pending",
            attempt_count: 0,
            last_error: null,
          };
          rows.push(row);
          return {
            one() {
              return { id: row.id };
            },
            toArray() {
              return [row];
            },
          };
        }
        if (/SET state = 'published'/i.test(text)) {
          const row = rows.find((value) => value.id === Number(params[0]));
          if (row) row.state = "published";
          return {
            one() {
              return {};
            },
            toArray() {
              return [];
            },
          };
        }
        if (text.includes("DELETE")) {
          const index = rows.findIndex(
            (value) => value.id === Number(params[0]),
          );
          if (index >= 0) rows.splice(index, 1);
          return {
            one() {
              return {};
            },
            toArray() {
              return [];
            },
          };
        }
        const selected = /WHERE id = \?/i.test(text)
          ? rows.filter((row) => row.id === Number(params[0]))
          : rows;
        return {
          one() {
            return selected[0] ?? {};
          },
          toArray() {
            return [...selected];
          },
        };
      },
    },
    async sync() {},
    rows,
  };
}

test("private Queue publisher normalizes native formats and metadata without exposing authority", async () => {
  const calls = [];
  const binding = {};
  const publisher = new QueuePublisher(
    binding,
    transport(async (operation, frame) => {
      calls.push([operation, frame]);
      return {
        backlogCount: 1,
        backlogBytes: 3,
        oldestMessageTimestampMs: 1000,
      };
    }),
    false,
    "EVENTS",
    codec,
  );
  const result = await publisher.send(bytes('{"ok":true}'), "json");
  assert.equal(result.metadata.metrics.oldestMessageTimestamp.getTime(), 1000);
  await publisher.send(bytes("plain"), "text", 0);
  await publisher.send(new Uint8Array([1, 2, 3]), "bytes");
  const graph = { when: new Date(42), map: new Map([["k", new Set([1])]]) };
  graph.self = graph;
  await publisher.send(serialize(graph), "v8", 9);
  assert.deepEqual(
    calls.map(([, frame]) => frameMessages(frame)[0].contentType),
    [1, 2, 3, 4],
  );
  assert.equal(frameMessages(calls[3][1])[0].delay, 9);
  await publisher.sendBatch(
    batch([
      [bytes("a"), "text"],
      [bytes("1"), "json"],
    ]),
    3,
  );
  assert.equal(calls[4][0], "batch");
  assert.equal(new DataView(calls[4][1].buffer).getInt32(7), 3);
  assert.equal((await publisher.metrics()).backlogCount, 1);
  assert.deepEqual(Reflect.ownKeys(binding), []);
  assert.equal(binding[FLUSH_OUTPUT], undefined);
  assert.equal(binding[FINALIZE_OUTPUT], undefined);
});

test("private Queue publisher rejects invalid wire and missing DO gate before authority writes", async () => {
  let writes = 0;
  const raw = transport(async () => {
    writes += 1;
    return { backlogCount: 0, backlogBytes: 0 };
  });
  for (const input of [null, {}, { send() {}, sendBatch() {}, metrics() {} }])
    assert.throws(
      () => new QueuePublisher({}, input, false, "EVENTS", codec),
      /QUEUE_INVARIANT_VIOLATION/,
    );
  const binding = {};
  const publisher = new QueuePublisher(binding, raw, false, "EVENTS", codec);
  assert.throws(
    () => new QueuePublisher(binding, raw, false, "EVENTS", codec),
    /DO_OUTPUT_GATE_UNPUBLISHABLE/,
  );
  await assert.rejects(
    publisher.send(bytes("x"), "xml"),
    /QUEUE_CONTENT_TYPE_UNSUPPORTED/,
  );
  for (const delay of [-1, 86401, 0.1, NaN])
    await assert.rejects(
      publisher.send(bytes("x"), "text", delay),
      /QUEUE_DELAY_INVALID/,
    );
  await assert.rejects(
    publisher.sendBatch("x".repeat(360001)),
    /QUEUE_BATCH_LIMIT_EXCEEDED/,
  );
  await assert.rejects(publisher.sendBatch("{"), /QUEUE_INVALID_MESSAGE/);
  await assert.rejects(
    publisher.sendBatch(batch([[bytes("x"), "text"]]), 86401),
    /QUEUE_DELAY_INVALID/,
  );
  const durable = new QueuePublisher({}, raw, true, "EVENTS", codec);
  await assert.rejects(
    durable.send(bytes("x"), "text"),
    /QUEUE_INVARIANT_VIOLATION/,
  );
  assert.equal(writes, 0);
});

test("native Queue publication preserves hosted validation error classes without writes", async () => {
  let writes = 0;
  const publisher = new QueuePublisher(
    {},
    transport(async () => {
      writes += 1;
      return { backlogCount: 0, backlogBytes: 0 };
    }),
    false,
    "EVENTS",
    codec,
  );
  const capture = async (action, name, code) => {
    await assert.rejects(action, (error) => {
      assert.equal(error.name, name);
      assert.equal(error.message, code);
      return true;
    });
    assert.equal(writes, 0);
  };
  await capture(
    () => publisher.send(bytes("x"), "xml"),
    "TypeError",
    "QUEUE_CONTENT_TYPE_UNSUPPORTED",
  );
  for (const delay of [-1, 86401]) {
    await capture(
      () => publisher.send(bytes("x"), "text", delay),
      "Error",
      "QUEUE_DELAY_INVALID",
    );
    await capture(
      () => publisher.sendBatch(batch([[bytes("x"), "text", delay]])),
      "Error",
      "QUEUE_DELAY_INVALID",
    );
    await capture(
      () => publisher.sendBatch(batch([[bytes("x"), "text"]]), delay),
      "Error",
      "QUEUE_DELAY_INVALID",
    );
  }
  await capture(
    () => publisher.sendBatch(batch([])),
    "TypeError",
    "QUEUE_BATCH_LIMIT_EXCEEDED",
  );
  for (const messages of [
    Array.from({ length: 101 }, () => [bytes("x"), "text"]),
    Array.from({ length: 3 }, () => [new Uint8Array(85500), "bytes"]),
    Array.from({ length: 3 }, () => [new Uint8Array(90000), "bytes"]),
  ])
    await capture(
      () => publisher.sendBatch(batch(messages)),
      "Error",
      "QUEUE_BATCH_LIMIT_EXCEEDED",
    );
  await publisher.sendBatch(batch([[bytes("x"), "text", 0]]), 86400);
  assert.equal(writes, 1);
});

test("Queue recovery privately preserves recorded frame and operation through finalization", async () => {
  const storage = gateStorage();
  const gate = new DoOutputGate(storage);
  const binding = {};
  const attempts = [];
  new QueuePublisher(
    binding,
    {
      async send(frame, id) {
        attempts.push(["send", frame, id]);
        return { backlogCount: 1, backlogBytes: 1 };
      },
      async sendBatch(frame, id) {
        attempts.push(["batch", frame, id]);
        return { backlogCount: 1, backlogBytes: 1 };
      },
      async finalize(id) {
        attempts.push(["finalize", id]);
      },
      async metrics() {
        throw Error("replay must not stage an intent");
      },
    },
    true,
    "EVENTS",
    codec,
  );
  for (const [index, frame] of [
    new Uint8Array([
      79, 67, 81, 49, 1, 0, 1, 255, 255, 255, 255, 2, 255, 255, 255, 255, 0, 0,
      0, 1, 97,
    ]),
    new Uint8Array([
      79, 67, 81, 49, 2, 0, 1, 255, 255, 255, 255, 2, 255, 255, 255, 255, 0, 0,
      0, 1, 98,
    ]),
  ].entries()) {
    const id = `aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaa${index}`;
    storage.rows.push({
      id: index + 1,
      kind: "queue",
      publisher: "EVENTS",
      payload: frame,
      operation_id: id,
      state: "pending",
    });
    await gate.recover({ EVENTS: binding });
    assert.deepEqual(attempts.at(-2), [
      index === 0 ? "send" : "batch",
      frame,
      id,
    ]);
    assert.deepEqual(attempts.at(-1), ["finalize", id]);
  }
  assert.equal(storage.rows.length, 0);
});

test("awaited DO Queue stages before commit and retains one identity through finalize", async () => {
  const storage = gateStorage();
  const gate = new DoOutputGate(storage);
  const operations = [];
  const publisher = new QueuePublisher(
    {},
    {
      async send(frame, id) {
        operations.push(["send", frame, id]);
        return { backlogCount: 4, backlogBytes: 7 };
      },
      async sendBatch(frame, id) {
        operations.push(["batch", frame, id]);
        return { backlogCount: 5, backlogBytes: 8 };
      },
      async finalize(id) {
        operations.push(["finalize", id]);
      },
      async metrics() {
        return { backlogCount: 3, backlogBytes: 4 };
      },
    },
    true,
    "EVENTS",
    codec,
  );
  gate.enterTransaction();
  const staged = await runWithOutputGate(gate, () =>
    publisher.send(bytes("x"), "text"),
  );
  assert.equal(staged.metadata.metrics.backlogCount, 4);
  assert.equal(staged.metadata.metrics.backlogBytes, 5);
  assert.ok(staged.metadata.metrics.oldestMessageTimestamp instanceof Date);
  assert.deepEqual(operations, []);
  const [intent] = storage.rows;
  await gate.exitTransaction("committed");
  assert.deepEqual(operations, [
    ["send", intent.payload, intent.operation_id],
    ["finalize", intent.operation_id],
  ]);
  assert.equal(storage.rows.length, 0);
  const actual = await runWithOutputGate(gate, () =>
    publisher.sendBatch(batch([[bytes("x"), "text"]])),
  );
  assert.equal(actual.metadata.metrics.backlogCount, 5);
  assert.equal(storage.rows.length, 0);
});

test("DO Queue finalize loss is recovered without republishing the acknowledged output", async () => {
  const storage = gateStorage();
  const gate = new DoOutputGate(storage);
  let published = 0;
  let failFinalize = true;
  const ids = [];
  const binding = {};
  const publisher = new QueuePublisher(
    binding,
    {
      async send(_frame, id) {
        ++published;
        ids.push(id);
        return { backlogCount: 1, backlogBytes: 1 };
      },
      async sendBatch() {
        throw Error("unexpected batch");
      },
      async finalize(id) {
        ids.push(id);
        if (failFinalize) throw Error("lost response");
      },
      async metrics() {
        return { backlogCount: 0, backlogBytes: 0 };
      },
    },
    true,
    "EVENTS",
    codec,
  );
  await assert.rejects(
    runWithOutputGate(gate, () => publisher.send(bytes("x"), "text")),
    /DO_OUTPUT_GATE_FINALIZE_FAILED/,
  );
  assert.equal(storage.rows[0].state, "published");
  failFinalize = false;
  await new DoOutputGate(storage).recover({ EVENTS: binding });
  assert.equal(published, 1);
  assert.equal(ids.length, 3);
  assert.ok(ids.every((id) => id === ids[0]));
  assert.equal(storage.rows.length, 0);
});

test("DO output gate retains original large V8 bytes through intent recovery", async () => {
  const storage = gateStorage();
  const gate = new DoOutputGate(storage);
  const binding = {};
  const calls = [];
  const raw = transport(async (operation, frame, id) => {
    calls.push({ operation, frame, id });
    return { backlogCount: 1, backlogBytes: 100006 };
  });
  const publisher = new QueuePublisher(binding, raw, true, "LARGE", codec);
  const body = serialize("你".repeat(50_000));
  gate.enterTransaction();
  await runWithOutputGate(gate, () => publisher.send(body, "v8"));
  assert.equal(storage.rows.length, 1);
  assert.equal(calls.filter((call) => call.operation === "send").length, 0);
  const intent = storage.rows[0];
  assert.deepEqual(frameMessages(intent.payload)[0].body, new Uint8Array(body));
  const recovered = new DoOutputGate(storage);
  await recovered.recover({ LARGE: binding });
  const published = calls.find((call) => call.operation === "send");
  assert.deepEqual(
    frameMessages(published.frame)[0].body,
    new Uint8Array(body),
  );
  assert.equal(published.id, intent.operation_id);
  assert.equal(calls.at(-1).operation, "finalize");
  assert.equal(storage.rows.length, 0);
});
