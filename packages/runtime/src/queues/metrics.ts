export interface QueueMetrics {
  backlogCount: number;
  backlogBytes: number;
  oldestMessageTimestamp?: Date;
}

function invariant(): never {
  throw Object.assign(new TypeError("QUEUE_INVARIANT_VIOLATION"), {
    stableCode: "QUEUE_INVARIANT_VIOLATION",
  });
}

export function queueMetrics(input: unknown): QueueMetrics {
  if (input === null || typeof input !== "object" || Array.isArray(input))
    invariant();
  const value = input as Record<string, unknown>;
  if (
    typeof value.backlogCount !== "number" ||
    !Number.isSafeInteger(value.backlogCount) ||
    value.backlogCount < 0 ||
    typeof value.backlogBytes !== "number" ||
    !Number.isSafeInteger(value.backlogBytes) ||
    value.backlogBytes < 0
  ) {
    invariant();
  }
  const output: QueueMetrics = {
    backlogCount: value.backlogCount,
    backlogBytes: value.backlogBytes,
  };
  const oldest = value.oldestMessageTimestampMs;
  if (oldest !== undefined && oldest !== null && oldest !== 0) {
    if (typeof oldest !== "number" || !Number.isSafeInteger(oldest)) {
      invariant();
    }
    output.oldestMessageTimestamp = new Date(oldest);
    if (!Number.isFinite(output.oldestMessageTimestamp.getTime())) invariant();
  }
  return output;
}
