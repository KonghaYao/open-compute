// The pinned workerd KvNamespace subrequest protocol, backed by the existing KV authority.
import { bindingJson } from "../bindings/json-body.js";
import type { KVNamespace } from "./transport.js";

const MAX_BULK_RESPONSE_BYTES = 25 * 1024 * 1024;
const MAX_PUT_BYTES = 25 * 1024 * 1024;

class KvInputError extends Error {
  constructor(
    readonly status: number,
    message: string,
  ) {
    super(message);
  }
}

async function drainRejectedPut(request: Request): Promise<void> {
  if (request.method !== "PUT" || !request.body || request.body.locked) return;
  // Native KV waits for its upload to complete before inspecting the status.
  // Drain a rejected valid-sized upload without allocating or writing its value.
  const reader = request.body.getReader();
  let size = 0;
  try {
    while (true) {
      const chunk = await reader.read();
      if (chunk.done) break;
      size += chunk.value.byteLength;
      if (size > MAX_PUT_BYTES) {
        await reader.cancel();
        break;
      }
    }
  } catch {
    // The original sanitized rejection remains authoritative on input failure.
  } finally {
    reader.releaseLock();
  }
}

// Count JSON string escapes and UTF-8 before allocating an expanded response.
function jsonStringBytes(value: string, remaining: number): number {
  let bytes = 2;
  for (let index = 0; index < value.length; index++) {
    const code = value.charCodeAt(index);
    if (code === 34 || code === 92) bytes += 2;
    else if (code < 32) bytes += [8, 9, 10, 12, 13].includes(code) ? 2 : 6;
    else if (code < 128) bytes++;
    else if (code < 2048) bytes += 2;
    else if (code >= 0xd800 && code <= 0xdbff) {
      const next = value.charCodeAt(index + 1);
      if (next >= 0xdc00 && next <= 0xdfff) {
        bytes += 4;
        index++;
      } else bytes += 6;
    } else bytes += code >= 0xdc00 && code <= 0xdfff ? 6 : 3;
    if (bytes > remaining) throw new TypeError("KV_BULK_TOO_LARGE");
  }
  return bytes;
}

function bulkResponse(values: Map<string, unknown>): Response {
  let remaining = MAX_BULK_RESPONSE_BYTES;
  // Transport values and metadata are JSON-derived; stringify owns traversal.
  const body = JSON.stringify(
    Object.fromEntries(values),
    (_, value: unknown) => {
      let bytes: number;
      if (typeof value === "string") bytes = jsonStringBytes(value, remaining);
      else if (value === null) bytes = 4;
      else if (typeof value === "boolean") bytes = value ? 4 : 5;
      else if (typeof value === "number") bytes = JSON.stringify(value).length;
      else if (Array.isArray(value)) bytes = 2 + Math.max(0, value.length - 1);
      else if (record(value)) {
        const keys = Object.keys(value);
        bytes = 2 + Math.max(0, keys.length - 1);
        for (const key of keys)
          bytes += 1 + jsonStringBytes(key, remaining - bytes);
      } else throw new Error("KV_INTERNAL_PROTOCOL_ERROR");
      remaining -= bytes;
      if (remaining < 0) throw new TypeError("KV_BULK_TOO_LARGE");
      return value;
    },
  );
  return new Response(body, {
    headers: { "content-type": "application/json" },
  });
}

function record(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

async function bulkOptions(request: Request): Promise<Record<string, unknown>> {
  const value = await bindingJson(request, 64 * 1024, "KV_INVALID_OPTIONS");
  if (
    !record(value) ||
    !Array.isArray(value.keys) ||
    value.keys.some((key: unknown) => typeof key !== "string") ||
    (value.type !== undefined && typeof value.type !== "string") ||
    (value.withMetadata !== undefined &&
      typeof value.withMetadata !== "boolean")
  ) {
    throw new TypeError("KV_INVALID_OPTIONS");
  }
  return value;
}

/** Translate native KV requests without exposing the transport or changing resource authority. */
export async function nativeKvFetch(
  request: Request,
  namespace: KVNamespace,
): Promise<Response> {
  try {
    const url = new URL(request.url);
    const options = url.searchParams;
    if (url.hostname !== "fake-host") throw new TypeError("KV_INVALID_OPTIONS");
    if (request.method === "POST" && url.pathname === "/bulk/get") {
      const input = await bulkOptions(request);
      // bulkOptions has validated the wire; validate key/count/options again at the authority.
      const keys = input.keys;
      if (!Array.isArray(keys)) throw new TypeError("KV_INVALID_OPTIONS");
      const names: string[] = [];
      for (const key of keys) {
        if (typeof key !== "string") throw new TypeError("KV_INVALID_OPTIONS");
        names.push(key);
      }
      if (names.length === 0)
        throw new KvInputError(400, "You must request a minimum of 1 key");
      if (names.length > 100)
        throw new KvInputError(400, "You can request a maximum of 100 keys");
      for (const name of names) {
        if (name === "" || name === "." || name === "..")
          throw new KvInputError(400, `Key name ${name} is not legal`);
        const length = new TextEncoder().encode(name).byteLength;
        if (length > 512)
          throw new KvInputError(
            414,
            `Encoded length of ${length} is too long`,
          );
      }
      if (
        input.type !== undefined &&
        input.type !== "text" &&
        input.type !== "json"
      )
        throw new KvInputError(
          400,
          `${JSON.stringify(input.type)} is not a valid type. Use "json" or "text"`,
        );
      if (input.cacheTtl !== undefined && Number(input.cacheTtl) < 30)
        throw new KvInputError(
          400,
          `Invalid cache_ttl of ${Number(input.cacheTtl)}. Cache TTL must be at least 30.`,
        );
      const selected = {
        type: input.type ?? "text",
        ...(input.cacheTtl === undefined
          ? {}
          : { cacheTtl: Number(input.cacheTtl) }),
      };
      const values =
        input.withMetadata === true
          ? await namespace.getWithMetadata(names, selected)
          : await namespace.get(names, selected);
      if (!(values instanceof Map))
        throw new Error("KV_INTERNAL_PROTOCOL_ERROR");
      return bulkResponse(values);
    }
    if (request.method === "GET" && url.pathname === "/") {
      if (
        options.has("key_count_limit") &&
        Number(options.get("key_count_limit")) > 1000
      )
        throw new KvInputError(
          400,
          `Invalid key_count_limit of ${Number(options.get("key_count_limit"))}. Please specify integer less than 1000.`,
        );
      const result = await namespace.list({
        ...(options.has("prefix") ? { prefix: options.get("prefix") } : {}),
        ...(options.has("cursor") ? { cursor: options.get("cursor") } : {}),
        ...(options.has("key_count_limit")
          ? { limit: Number(options.get("key_count_limit")) }
          : {}),
      });
      return Response.json({
        ...result,
        keys: result.keys.map((key) => ({
          ...key,
          ...(key.metadata === undefined
            ? {}
            : { metadata: JSON.stringify(key.metadata) }),
        })),
      });
    }
    if (options.get("urlencoded") !== "true")
      throw new TypeError("KV_INVALID_OPTIONS");
    const key = decodeURIComponent(url.pathname.slice(1));
    if (request.method === "GET") {
      if (options.has("cache_ttl") && Number(options.get("cache_ttl")) < 30)
        throw new KvInputError(
          400,
          `Invalid cache_ttl of ${Number(options.get("cache_ttl"))}. Cache TTL must be at least 30.`,
        );
      const entry = await namespace.getWithMetadata(key, {
        type: "stream",
        ...(options.has("cache_ttl")
          ? { cacheTtl: Number(options.get("cache_ttl")) }
          : {}),
      });
      if (entry instanceof Map) throw new Error("KV_INTERNAL_PROTOCOL_ERROR");
      if (entry.value === null) return new Response(null, { status: 404 });
      if (!(entry.value instanceof ReadableStream))
        throw new Error("KV_INTERNAL_PROTOCOL_ERROR");
      return new Response(entry.value, {
        headers:
          entry.metadata === null
            ? {}
            : {
                "cf-kv-metadata": JSON.stringify(entry.metadata),
              },
      });
    }
    if (request.method === "PUT") {
      const metadata = request.headers.get("cf-kv-metadata");
      if (options.has("expiration_ttl")) {
        const ttl = Number(options.get("expiration_ttl"));
        if (ttl < 60)
          throw new KvInputError(
            400,
            `Invalid expiration_ttl of ${ttl}. Expiration TTL must be at least 60.`,
          );
      } else if (options.has("expiration")) {
        const expiration = Number(options.get("expiration"));
        if (expiration < Math.floor(Date.now() / 1000) + 60)
          throw new KvInputError(
            400,
            `Invalid expiration of ${expiration}. Please specify integer greater than the current number of seconds since the UNIX epoch.`,
          );
      }
      if (
        metadata !== null &&
        new TextEncoder().encode(metadata).byteLength > 1024
      )
        throw new KvInputError(413, "Payload Too Large");
      await namespace.put(key, request.body ?? new Uint8Array(), {
        ...(options.has("expiration_ttl")
          ? { expirationTtl: Number(options.get("expiration_ttl")) }
          : options.has("expiration")
            ? { expiration: Number(options.get("expiration")) }
            : {}),
        ...(metadata === null
          ? {}
          : { metadata: JSON.parse(metadata) as unknown }),
      });
      return new Response(null, { status: 204 });
    }
    if (request.method === "DELETE") {
      await namespace.delete(key);
      return new Response(null, { status: 204 });
    }
    throw new TypeError("KV_INVALID_OPTIONS");
  } catch (error) {
    await drainRejectedPut(request);
    if (error instanceof KvInputError)
      return new Response(null, {
        status: error.status,
        statusText: error.message,
      });
    if (error instanceof URIError)
      return new Response(null, {
        status: 400,
        statusText: "Could not URL-decode key name",
      });
    if (error instanceof SyntaxError && request.method === "POST")
      return new Response(null, {
        status: 500,
        statusText: "Internal Server Error",
      });
    const reported =
      error instanceof TypeError
        ? error.message
        : error instanceof Error
          ? Object.getOwnPropertyDescriptor(error, "stableCode")?.value
          : undefined;
    const code =
      typeof reported === "string" &&
      /^(?:KV|BINDING|RESOURCE)_[A-Z0-9_]{1,127}$/.test(reported)
        ? reported
        : "KV_INTERNAL_PROTOCOL_ERROR";
    return new Response(null, {
      status:
        code === "BINDING_PERMISSION_DENIED"
          ? 403
          : code === "KV_CURSOR_INVALID"
            ? 400
            : code === "KV_BULK_TOO_LARGE"
              ? 413
              : error instanceof TypeError
                ? 400
                : 500,
      statusText: code === "KV_CURSOR_INVALID" ? "Invalid cursor" : code,
    });
  }
}
