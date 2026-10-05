export interface QueryResult {
  columns: string[];
  rows: (null | string | number | number[])[][];
  meta: Record<string, unknown>;
}
const CORE_META = [
  "duration",
  "size_after",
  "rows_read",
  "rows_written",
  "last_row_id",
  "changed_db",
  "changes",
];
const OPTIONAL_META = [
  "served_by_region",
  "served_by_colo",
  "served_by_primary",
  "timings",
  "total_attempts",
];

function typeError(code: string): never {
  throw new TypeError(code);
}
function isRecord(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}
function finiteNumber(value: unknown): value is number {
  return typeof value === "number" && Number.isFinite(value);
}

function assertMeta(meta: unknown): Record<string, unknown> {
  if (!isRecord(meta)) typeError("D1_INTERNAL_PROTOCOL_ERROR");
  const allowed = new Set([...CORE_META, ...OPTIONAL_META]);
  if (
    Object.keys(meta).some((key) => !allowed.has(key)) ||
    CORE_META.some((key) => !Object.prototype.hasOwnProperty.call(meta, key)) ||
    typeof meta.changed_db !== "boolean" ||
    CORE_META.filter((key) => key !== "changed_db").some(
      (key) => !finiteNumber(meta[key]),
    )
  ) {
    typeError("D1_INTERNAL_PROTOCOL_ERROR");
  }
  if (
    meta.served_by_region !== undefined &&
    typeof meta.served_by_region !== "string"
  ) {
    typeError("D1_INTERNAL_PROTOCOL_ERROR");
  }
  if (
    meta.served_by_colo !== undefined &&
    typeof meta.served_by_colo !== "string"
  ) {
    typeError("D1_INTERNAL_PROTOCOL_ERROR");
  }
  if (
    meta.served_by_primary !== undefined &&
    typeof meta.served_by_primary !== "boolean"
  ) {
    typeError("D1_INTERNAL_PROTOCOL_ERROR");
  }
  if (meta.total_attempts !== undefined && !finiteNumber(meta.total_attempts)) {
    typeError("D1_INTERNAL_PROTOCOL_ERROR");
  }
  if (meta.timings !== undefined) {
    if (
      !isRecord(meta.timings) ||
      Object.keys(meta.timings).some((key) => key !== "sql_duration_ms") ||
      !finiteNumber(meta.timings.sql_duration_ms)
    ) {
      typeError("D1_INTERNAL_PROTOCOL_ERROR");
    }
  }
  return meta;
}

function outputValue(value: unknown): null | string | number | number[] {
  if (
    value === null ||
    typeof value === "string" ||
    (typeof value === "number" && Number.isFinite(value))
  )
    return value;
  if (value instanceof Uint8Array) return Array.from(value);
  if (value instanceof ArrayBuffer) return Array.from(new Uint8Array(value));
  typeError("D1_INTERNAL_PROTOCOL_ERROR");
}

/** Validate private authority output before exposing values through either upstream wire. */
export function assertQueryResponse(
  result: unknown,
  session: boolean,
): { results: QueryResult[]; bookmark: string | null; stateVersion: number } {
  if (
    !isRecord(result) ||
    !Array.isArray(result.results) ||
    !Number.isSafeInteger(result.stateVersion) ||
    (result.stateVersion as number) < 0
  ) {
    typeError("D1_INTERNAL_PROTOCOL_ERROR");
  }
  const results = result.results.map((entry: unknown) => {
    if (
      !isRecord(entry) ||
      !Array.isArray(entry.columns) ||
      !Array.isArray(entry.rows) ||
      entry.columns.length > 100 ||
      entry.columns.some((name) => typeof name !== "string")
    ) {
      typeError("D1_INTERNAL_PROTOCOL_ERROR");
    }
    const columns = entry.columns as string[];
    const rows = entry.rows.map((row: unknown) => {
      if (!Array.isArray(row) || row.length !== columns.length) {
        typeError("D1_INTERNAL_PROTOCOL_ERROR");
      }
      return row.map(outputValue);
    });
    return { columns: columns.slice(), rows, meta: assertMeta(entry.meta) };
  });
  if (session) {
    if (typeof result.bookmark !== "string" || result.bookmark.length === 0) {
      typeError("D1_INTERNAL_PROTOCOL_ERROR");
    }
  } else if (result.bookmark != null && result.bookmark !== "") {
    typeError("D1_INTERNAL_PROTOCOL_ERROR");
  }
  return {
    results,
    bookmark: session ? (result.bookmark as string) : null,
    stateVersion: result.stateVersion as number,
  };
}
