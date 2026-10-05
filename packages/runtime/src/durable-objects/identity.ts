import type { DoObjectIdentity } from "./protocol.js";

export const OBJECT_IDENTITY_HEADER = "x-open-compute-object-identity";
const encoder = new TextEncoder();
const jurisdictions = new Set(["eu", "us", "fedramp", "fedramp-high"]);

/** Validate display metadata independently of the authoritative binding resolution. */
export function objectIdentity(value: unknown): DoObjectIdentity {
  if (value === null || typeof value !== "object" || Array.isArray(value))
    throw new TypeError("DO_ID_INVALID");
  if (
    !("value" in value) ||
    typeof value.value !== "string" ||
    !/^[0-9a-f]{64}$/.test(value.value)
  )
    throw new TypeError("DO_ID_INVALID");
  const name = "name" in value ? value.name : undefined;
  const jurisdiction = "jurisdiction" in value ? value.jurisdiction : undefined;
  if (
    name !== undefined &&
    (typeof name !== "string" || encoder.encode(name).byteLength > 1024)
  )
    throw new TypeError("DO_ID_INVALID");
  if (
    jurisdiction !== undefined &&
    (typeof jurisdiction !== "string" || !jurisdictions.has(jurisdiction))
  )
    throw new TypeError("DO_ID_INVALID");
  return Object.freeze({ value: value.value, name, jurisdiction });
}

/** ASCII-only header carrying the namespace policy's captured ID metadata. */
export function encodeObjectIdentity(value: DoObjectIdentity): string {
  return encodeURIComponent(JSON.stringify(objectIdentity(value)));
}

/** Decode bounded metadata and require it to match the resolved object ID. */
export function identityFromHeaders(
  headers: Headers,
  value: string,
): DoObjectIdentity {
  const wire = headers.get(OBJECT_IDENTITY_HEADER);
  if (wire === null) return objectIdentity({ value });
  if (wire.length > 10_000) throw new TypeError("DO_ID_INVALID");
  let decoded: unknown;
  try {
    decoded = JSON.parse(decodeURIComponent(wire));
  } catch {
    throw new TypeError("DO_ID_INVALID");
  }
  const identity = objectIdentity(decoded);
  if (identity.value !== value) throw new TypeError("DO_ID_INVALID");
  return identity;
}
