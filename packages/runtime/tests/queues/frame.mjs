import assert from "node:assert/strict";

export function queueFrame(bytes) {
  assert.equal(new TextDecoder().decode(bytes.slice(0, 4)), "OCQ1");
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const count = view.getUint16(5);
  const messages = [];
  let offset = 11;
  for (let index = 0; index < count; index++) {
    const length = view.getUint32(offset + 5);
    messages.push({
      contentType: bytes[offset],
      delay: view.getInt32(offset + 1),
      body: bytes.slice(offset + 9, offset + 9 + length),
    });
    offset += 9 + length;
  }
  assert.equal(offset, bytes.byteLength);
  return { operation: view.getUint8(4), delay: view.getInt32(7), messages };
}

export function frameMessages(bytes) {
  return queueFrame(bytes).messages;
}
