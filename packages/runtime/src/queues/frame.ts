export interface SerializedMessage {
  contentType: "json" | "text" | "bytes" | "v8";
  bytes: Uint8Array;
  delaySeconds?: number | undefined;
}

function contentCode(value: SerializedMessage["contentType"]): number {
  if (value === "json") return 1;
  if (value === "text") return 2;
  if (value === "bytes") return 3;
  if (value === "v8") return 4;
  throw new TypeError("QUEUE_CONTENT_TYPE_UNSUPPORTED");
}

export function encodeQueueFrame(
  messages: readonly SerializedMessage[],
  batchDelay: number | undefined,
  operation: number,
): Uint8Array {
  let length = 11;
  for (const message of messages) length += 9 + message.bytes.byteLength;
  const output = new Uint8Array(length);
  output.set([0x4f, 0x43, 0x51, 0x31], 0);
  const view = new DataView(output.buffer);
  view.setUint8(4, operation);
  view.setUint16(5, messages.length);
  view.setInt32(7, batchDelay === undefined ? -1 : batchDelay);
  let offset = 11;
  for (const message of messages) {
    view.setUint8(offset, contentCode(message.contentType));
    view.setInt32(
      offset + 1,
      message.delaySeconds === undefined ? -1 : message.delaySeconds,
    );
    view.setUint32(offset + 5, message.bytes.byteLength);
    output.set(message.bytes, offset + 9);
    offset += 9 + message.bytes.byteLength;
  }
  return output;
}
