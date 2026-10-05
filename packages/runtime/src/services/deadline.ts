import { bindingError } from "../loader/shared.js";

const NativeAbortController = AbortController;
const nativeAbort = AbortController.prototype.abort;
const nativeApply = Reflect.apply;
const nativeRace = Promise.race.bind(Promise);

/** Normalize a private Service receipt's bounded duration to one absolute deadline. */
export function serviceDeadlineAt(deadlineMs: unknown): number {
  if (
    typeof deadlineMs !== "number" ||
    !Number.isSafeInteger(deadlineMs) ||
    deadlineMs < 1 ||
    deadlineMs > 30_000
  ) {
    throw bindingError("SERVICE_UNAVAILABLE");
  }
  return Date.now() + deadlineMs;
}

/** Bound admitted work without leaving a timer alive after success or failure. */
export async function serviceDeadline<T>(
  operation: () => Promise<T>,
  deadlineAt: number,
  timedOut?: () => void,
): Promise<T> {
  const remaining = deadlineAt - Date.now();
  if (remaining < 1) {
    timedOut?.();
    throw bindingError("SERVICE_TIMEOUT");
  }
  const timer = new NativeAbortController();
  try {
    return await nativeRace([
      scheduler.wait(remaining, { signal: timer.signal }).then(() => {
        timedOut?.();
        throw bindingError("SERVICE_TIMEOUT");
      }),
      // Turn synchronous invocation failures into a rejection before aborting the timer.
      (async () => operation())(),
    ]);
  } finally {
    nativeApply(nativeAbort, timer, []);
  }
}
