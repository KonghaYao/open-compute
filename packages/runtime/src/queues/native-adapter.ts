// workerd's native Queue subrequest wire over the existing durable Queue authority.
import { bindingJson } from "../bindings/json-body.js";
import { encodeQueueFrame, type SerializedMessage } from "./frame.js";
import { queueMetrics } from "./metrics.js";

interface QueueAuthority {
  send(frame: Uint8Array): Promise<unknown>;
  sendBatch(frame: Uint8Array): Promise<unknown>;
  metrics(): Promise<unknown>;
}
export interface QueueWireCodec {
  decodeV8(bytes: Uint8Array): unknown;
}

function fail(code = "QUEUE_INVALID_MESSAGE"): never {
  throw new TypeError(code);
}
export function queueDelay(value: unknown): number | undefined {
  if (value === undefined) return undefined;
  if (typeof value !== "number" || !Number.isSafeInteger(value))
    fail("QUEUE_DELAY_INVALID");
  if (value < 0 || value > 86400) throw new Error("QUEUE_DELAY_INVALID");
  return value;
}
function record(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}
export function nativeQueueMessage(
  bytes: Uint8Array,
  format: unknown,
  codec: QueueWireCodec,
): SerializedMessage {
  if (bytes.byteLength > 128000) fail("QUEUE_MESSAGE_TOO_LARGE");
  const contentType = format === undefined ? "v8" : format;
  if (
    contentType !== "json" &&
    contentType !== "text" &&
    contentType !== "bytes" &&
    contentType !== "v8"
  )
    fail("QUEUE_CONTENT_TYPE_UNSUPPORTED");
  if (contentType === "v8") {
    let value: unknown;
    try {
      value = codec.decodeV8(bytes);
    } catch {
      fail("QUEUE_V8_MALFORMED");
    }
    if (value === undefined) fail();
  } else if (contentType === "json" || contentType === "text") {
    try {
      const text = new TextDecoder("utf-8", {
        fatal: true,
        ignoreBOM: false,
      }).decode(bytes);
      if (contentType === "json") JSON.parse(text);
    } catch {
      fail();
    }
  }
  return { contentType, bytes };
}

/** Validate and normalize the native batch before any authority mutation. */
export function nativeQueueBatch(input: unknown, codec: QueueWireCodec) {
  if (
    !record(input) ||
    Object.keys(input).some((key) => key !== "messages") ||
    !Array.isArray(input.messages) ||
    !input.messages.length
  )
    fail("QUEUE_BATCH_LIMIT_EXCEEDED");
  if (input.messages.length > 100)
    throw new Error("QUEUE_BATCH_LIMIT_EXCEEDED");
  const messages: SerializedMessage[] = [];
  let wireSize = 0;
  let largest = 0;
  for (const item of input.messages) {
    if (
      !record(item) ||
      Object.keys(item).some(
        (key) => !["body", "contentType", "delaySecs"].includes(key),
      ) ||
      typeof item.body !== "string"
    )
      fail();
    let binary: string;
    try {
      binary = atob(item.body);
      if (btoa(binary) !== item.body) fail();
    } catch {
      fail();
    }
    const bytes = Uint8Array.from(binary, (char) => char.charCodeAt(0));
    wireSize += bytes.byteLength;
    largest = Math.max(largest, bytes.byteLength);
    if (wireSize > 256000) throw new Error("QUEUE_BATCH_LIMIT_EXCEEDED");
    const value = nativeQueueMessage(bytes, item.contentType, codec);
    value.delaySeconds = queueDelay(item.delaySecs);
    messages.push(value);
  }
  return { messages, wireSize, largest };
}

function metrics(raw: unknown) {
  const value = queueMetrics(raw);
  return {
    backlogCount: value.backlogCount,
    backlogBytes: value.backlogBytes,
    oldestMessageTimestamp: value.oldestMessageTimestamp?.getTime() ?? 0,
  };
}

const ERROR_CODES = new Set([
  "QUEUE_INVALID_MESSAGE",
  "QUEUE_DELAY_INVALID",
  "QUEUE_CONTENT_TYPE_UNSUPPORTED",
  "QUEUE_MESSAGE_TOO_LARGE",
  "QUEUE_BATCH_LIMIT_EXCEEDED",
  "QUEUE_V8_MALFORMED",
  "QUEUE_NOT_FOUND",
  "QUEUE_NOT_READY",
  "QUEUE_CONFIG_PENDING",
  "QUEUE_SEND_RESULT_UNKNOWN",
  "BINDING_PERMISSION_DENIED",
  "QUEUE_STORAGE_UNAVAILABLE",
  "QUEUE_INVARIANT_VIOLATION",
  "QUEUE_BACKLOG_LIMIT_EXCEEDED",
]);

/** Normalize native wire once before invoking the same immutable binding transport. */
export async function nativeQueueFetch(
  request: Request,
  authority: QueueAuthority,
  codec: QueueWireCodec,
): Promise<Response> {
  try {
    const url = new URL(request.url);
    if (
      url.origin !== "https://fake-host" ||
      url.username ||
      url.password ||
      url.search ||
      url.hash
    )
      fail();
    if (request.method === "GET" && url.pathname === "/metrics")
      return Response.json(metrics(await authority.metrics()));
    if (request.method !== "POST") fail();
    const delayHeader = request.headers.get("x-msg-delay-secs");
    if (delayHeader !== null && !/^(0|[1-9][0-9]{0,4})$/.test(delayHeader))
      fail("QUEUE_DELAY_INVALID");
    const batchDelay = queueDelay(
      delayHeader === null ? undefined : Number(delayHeader),
    );
    let raw: unknown;
    if (url.pathname === "/message") {
      let size = 0;
      const body = request.body?.pipeThrough(
        new TransformStream<Uint8Array, Uint8Array>({
          transform(chunk, controller) {
            size += chunk.byteLength;
            if (size > 128000) fail("QUEUE_MESSAGE_TOO_LARGE");
            controller.enqueue(chunk);
          },
        }),
      );
      const bytes = new Uint8Array(await new Response(body).arrayBuffer());
      const input = nativeQueueMessage(
        bytes,
        request.headers.get("x-msg-fmt") ?? undefined,
        codec,
      );
      input.delaySeconds = batchDelay;
      raw = await authority.send(encodeQueueFrame([input], undefined, 1));
    } else if (url.pathname === "/batch") {
      const input = await bindingJson(
        request,
        360000,
        "QUEUE_BATCH_LIMIT_EXCEEDED",
      );
      const { messages, wireSize, largest } = nativeQueueBatch(input, codec);
      for (const [header, expected] of [
        ["cf-queue-batch-count", messages.length],
        ["cf-queue-batch-bytes", wireSize],
        ["cf-queue-largest-msg", largest],
      ] as const) {
        const actual = request.headers.get(header);
        if (actual !== null && actual !== String(expected)) fail();
      }
      raw = await authority.sendBatch(
        encodeQueueFrame(messages, batchDelay, 2),
      );
    } else fail();
    return Response.json({ metadata: { metrics: metrics(raw) } });
  } catch (cause) {
    return nativeQueueFailure(cause);
  }
}

/** Return one stable native Queue error without exposing authority details. */
export function nativeQueueFailure(cause: unknown): Response {
  const candidate = cause instanceof Error ? cause.message : undefined;
  const code =
    candidate && ERROR_CODES.has(candidate)
      ? candidate
      : "QUEUE_STORAGE_UNAVAILABLE";
  return new Response(code, {
    status: 500,
    headers: {
      "cf-queues-error-cause": code,
      "cf-queues-error-code": "15000",
    },
  });
}
