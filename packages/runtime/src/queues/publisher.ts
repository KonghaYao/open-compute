import {
  currentOutputGate,
  FINALIZE_OUTPUT,
  FLUSH_OUTPUT,
  registerOutputPublisher,
} from "../durable-objects/output-gate.js";
import { encodeQueueFrame } from "./frame.js";
import { queueMetrics } from "./metrics.js";
import {
  nativeQueueBatch,
  nativeQueueMessage,
  queueDelay,
  type QueueWireCodec,
} from "./native-adapter.js";

interface QueueRawTransport {
  send(frame: Uint8Array, operationId?: string): Promise<unknown>;
  sendBatch(frame: Uint8Array, operationId?: string): Promise<unknown>;
  finalize(operationId: string): Promise<void>;
  metrics(): Promise<unknown>;
}

function typeError(code: string): never {
  throw Object.assign(new TypeError(code), { stableCode: code });
}

function response(value: unknown) {
  return { metadata: { metrics: queueMetrics(value) } };
}

async function stagedResponse(raw: QueueRawTransport, bytes: Uint8Array) {
  const current = queueMetrics(await raw.metrics());
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const count = view.getUint16(5);
  let offset = 11;
  let addedBytes = 0;
  for (let index = 0; index < count; index += 1) {
    const length = view.getUint32(offset + 5);
    addedBytes += length;
    offset += 9 + length;
  }
  return {
    metadata: {
      metrics: {
        backlogCount: current.backlogCount + count,
        backlogBytes: current.backlogBytes + addedBytes,
        oldestMessageTimestamp: current.oldestMessageTimestamp ?? new Date(),
      },
    },
  };
}

/** Private native Queue publication policy; never exposed as the tenant binding. */
export class QueuePublisher {
  #raw: QueueRawTransport;
  #durableObject: boolean;
  #name: string;
  #codec: QueueWireCodec;

  constructor(
    binding: object,
    raw: unknown,
    durableObject: boolean,
    name: string,
    codec: QueueWireCodec,
  ) {
    if (!rawTransport(raw)) typeError("QUEUE_INVARIANT_VIOLATION");
    this.#raw = raw;
    this.#durableObject = durableObject;
    this.#name = name;
    this.#codec = codec;
    registerOutputPublisher(binding, this);
  }

  async #publish(bytes: Uint8Array, batch: boolean) {
    const send = (operationId?: string) =>
      batch
        ? this.#raw.sendBatch(bytes, operationId)
        : this.#raw.send(bytes, operationId);
    if (!this.#durableObject) return response(await send());
    const gate = currentOutputGate();
    if (!gate) typeError("QUEUE_INVARIANT_VIOLATION");
    return gate.schedule(
      "queue",
      this.#name,
      bytes,
      async (operationId) => response(await send(operationId)),
      () => stagedResponse(this.#raw, bytes),
      (operationId) => this.#raw.finalize(operationId),
    );
  }

  [FLUSH_OUTPUT](payload: Uint8Array, operationId: string) {
    const send =
      payload[4] === 2
        ? this.#raw.sendBatch(payload, operationId)
        : this.#raw.send(payload, operationId);
    return send.then(response);
  }

  [FINALIZE_OUTPUT](operationId: string) {
    return this.#raw.finalize(operationId);
  }

  async send(
    bytes: Uint8Array,
    contentType: string | undefined,
    delaySeconds: number | undefined,
  ) {
    const message = nativeQueueMessage(bytes, contentType, this.#codec);
    message.delaySeconds = queueDelay(delaySeconds);
    return this.#publish(encodeQueueFrame([message], undefined, 1), false);
  }

  async sendBatch(body: string, delaySeconds: number | undefined) {
    if (body.length > 360000) throw new Error("QUEUE_BATCH_LIMIT_EXCEEDED");
    let input: unknown;
    try {
      input = JSON.parse(body);
    } catch {
      typeError("QUEUE_INVALID_MESSAGE");
    }
    const { messages } = nativeQueueBatch(input, this.#codec);
    return this.#publish(
      encodeQueueFrame(messages, queueDelay(delaySeconds), 2),
      true,
    );
  }

  async metrics() {
    return queueMetrics(await this.#raw.metrics());
  }
}

function rawTransport(raw: unknown): raw is QueueRawTransport {
  return (
    raw !== null &&
    typeof raw === "object" &&
    "send" in raw &&
    typeof raw.send === "function" &&
    "sendBatch" in raw &&
    typeof raw.sendBatch === "function" &&
    "finalize" in raw &&
    typeof raw.finalize === "function" &&
    "metrics" in raw &&
    typeof raw.metrics === "function"
  );
}
