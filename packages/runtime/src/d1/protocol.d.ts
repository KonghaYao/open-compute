/** Values accepted by the binary D1 protocol after tenant-side normalization. */
export type D1Value = null | string | number | Uint8Array;
export type D1QueryMode = "all" | "run" | "raw" | "batch";
export interface D1StatementDto {
  sql: string;
  params: readonly D1Value[];
}

/** Session constraint or sealed bookmark sent with a terminal D1 query. */
export interface D1SessionWire {
  kind: 0 | 1 | 2 | 3;
  bookmark?: string;
}

/** Canonical response decoded from the instance-owned D1 backend. */
export interface D1QueryResponse {
  results: { columns: string[]; rows: D1Value[][]; meta: unknown }[];
  bookmark: string | null;
  stateVersion: number;
}

/** Private authority used by the native D1 wire adapter. */
export interface D1RawTransport {
  executeStatements(
    mode: D1QueryMode,
    statements: readonly D1StatementDto[],
    session?: D1SessionWire,
  ): Promise<D1QueryResponse>;
  exec(sql: string, options?: Record<string, never>): Promise<unknown>;
}
