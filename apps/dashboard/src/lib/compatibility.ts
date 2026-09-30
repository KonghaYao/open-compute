/** Latest compatibility date that workerd can accept on the current UTC day. */
export function latestCompatibilityDate(
  binaryMaximum: string,
  today = new Date().toISOString().slice(0, 10),
) {
  return binaryMaximum < today ? binaryMaximum : today;
}
