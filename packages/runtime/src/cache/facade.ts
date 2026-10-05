const nativeGet = Reflect.get;
const nativeFreeze = Object.freeze;
const nativeDescriptor = Object.getOwnPropertyDescriptor;

const PRIVATE_CACHE = "__OPEN_COMPUTE_PRIVATE_CACHE";
const CACHEABLE_STATUS = new Set([
  200, 203, 204, 300, 301, 404, 405, 410, 414, 501,
]);

interface PurgeOptions {
  tags?: string[];
  pathPrefixes?: string[];
  purgeEverything?: boolean;
}
type CacheLookupStatus =
  "HIT" | "MISS" | "EXPIRED" | "UPDATING" | "STALE" | "STALE_IF_ERROR";
interface CacheWriteFence {
  fenceGeneration: string;
  refreshToken?: string;
}
interface CacheLookup extends CacheWriteFence {
  status: CacheLookupStatus;
  response?: Response;
}
interface CacheTransport {
  match(
    namespace: "automatic" | "default" | "named",
    name: string | undefined,
    request: Request,
  ): Promise<CacheLookup>;
  putAutomatic(
    request: Request,
    response: Response,
    fence: CacheWriteFence,
  ): Promise<void>;
  purge(options: PurgeOptions): Promise<{ success: boolean; deleted: number }>;
}

const LOOKUP_STATUSES = new Set<CacheLookupStatus>([
  "HIT",
  "MISS",
  "EXPIRED",
  "UPDATING",
  "STALE",
  "STALE_IF_ERROR",
]);
const RESPONSE_LOOKUP_STATUSES = new Set<CacheLookupStatus>([
  "HIT",
  "UPDATING",
  "STALE",
  "STALE_IF_ERROR",
]);

function bindTransport(
  environment: object,
  entrypoint: string,
): CacheTransport {
  const transports: unknown = nativeGet(environment, PRIVATE_CACHE);
  const value: unknown =
    transports !== null && typeof transports === "object"
      ? nativeGet(transports, entrypoint)
      : undefined;
  if (
    value === null ||
    typeof value !== "object" ||
    typeof nativeGet(value, "match") !== "function" ||
    typeof nativeGet(value, "putAutomatic") !== "function" ||
    typeof nativeGet(value, "purge") !== "function"
  ) {
    throw new Error("CACHE_UNAVAILABLE");
  }
  return value as CacheTransport;
}

function cacheLookup(value: unknown): CacheLookup {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    throw new TypeError("CACHE_PROTOCOL_ERROR");
  }
  const status: unknown = nativeGet(value, "status");
  const fenceGeneration: unknown = nativeGet(value, "fenceGeneration");
  const refreshToken: unknown = nativeGet(value, "refreshToken");
  const response: unknown = nativeGet(value, "response");
  if (
    typeof status !== "string" ||
    !LOOKUP_STATUSES.has(status as CacheLookupStatus) ||
    typeof fenceGeneration !== "string" ||
    !/^[1-9][0-9]{0,19}$/.test(fenceGeneration) ||
    (refreshToken !== undefined &&
      (typeof refreshToken !== "string" ||
        !/^[0-9a-f]{32}$/.test(refreshToken))) ||
    (status === "UPDATING") !== (refreshToken !== undefined) ||
    (response !== undefined && !(response instanceof Response)) ||
    RESPONSE_LOOKUP_STATUSES.has(status as CacheLookupStatus) !==
      (response !== undefined)
  ) {
    throw new TypeError("CACHE_PROTOCOL_ERROR");
  }
  return value as CacheLookup;
}

export interface CacheRuntime {
  readonly context: {
    purge(
      options: PurgeOptions,
    ): Promise<{ success: boolean; deleted: number }>;
  };
  dispatch(
    origin: () => unknown,
    request: Request,
    ctx: ExecutionContext,
  ): Promise<Response>;
}

export interface CacheRuntimeFactory {
  bind(): CacheRuntime | undefined;
}

function cacheable(request: Request, response: Response): boolean {
  const requestControl =
    request.headers.get("cache-control")?.toLowerCase() ?? "";
  const responseControl = (
    response.headers.get("cloudflare-cdn-cache-control") ??
    response.headers.get("cdn-cache-control") ??
    response.headers.get("cache-control") ??
    ""
  ).toLowerCase();
  return (
    request.method === "GET" &&
    !request.headers.has("authorization") &&
    !response.headers.has("set-cookie") &&
    CACHEABLE_STATUS.has(response.status) &&
    !hasDirective(
      `${requestControl},${responseControl}`,
      new Set(["no-store", "no-cache", "private"]),
    ) &&
    hasExplicitTtl(responseControl)
  );
}

function hasDirective(value: string, names: ReadonlySet<string>): boolean {
  return value.split(",").some((part) => {
    const equals = part.indexOf("=");
    const name = (equals === -1 ? part : part.slice(0, equals)).trim();
    return names.has(name);
  });
}

function hasExplicitTtl(value: string): boolean {
  return value.split(",").some((part) => {
    const equals = part.indexOf("=");
    if (equals === -1) return false;
    const name = part.slice(0, equals).trim();
    const seconds = part.slice(equals + 1).trim();
    return (
      ["s-maxage", "max-age"].includes(name) &&
      /^(?:[0-9]+|"[0-9]+")$/.test(seconds)
    );
  });
}

function withCacheStatus(response: Response, status: string): Response {
  const result = new Response(response.body, response);
  result.headers.delete("cache-tag");
  result.headers.set("cf-cache-status", status);
  return result;
}

async function originResponse(origin: () => unknown): Promise<Response> {
  const response = await origin();
  if (!(response instanceof Response))
    throw new TypeError("CACHE_PROTOCOL_ERROR");
  return response;
}

async function discardResponse(response: Response): Promise<void> {
  try {
    await response.body?.cancel();
  } catch {
    // The response is already hidden from the tenant; cancellation is best effort.
  }
}

function cacheFailureCode(error: unknown): string {
  if (error === null || typeof error !== "object")
    return "CACHE_PROTOCOL_ERROR";
  for (const key of ["stableCode", "message"]) {
    let descriptor: PropertyDescriptor | undefined;
    try {
      descriptor = nativeDescriptor(error, key);
    } catch {
      return "CACHE_PROTOCOL_ERROR";
    }
    if (
      typeof descriptor?.value === "string" &&
      [
        "CACHE_UNAVAILABLE",
        "CACHE_RESULT_UNKNOWN",
        "CACHE_CORRUPT",
        "CACHE_PROTOCOL_ERROR",
      ].includes(descriptor.value)
    )
      return descriptor.value;
  }
  return "CACHE_PROTOCOL_ERROR";
}

/** Build the automatic dispatcher only for an explicitly enabled fetch entrypoint. */
export function createCacheRuntime(
  enabled: boolean,
  failOpen: boolean,
  privateEnvironment: object,
  entrypoint = "default",
): CacheRuntimeFactory {
  return nativeFreeze({
    bind(): CacheRuntime | undefined {
      if (!enabled) return undefined;
      const raw = bindTransport(privateEnvironment, entrypoint);
      return nativeFreeze({
        context: nativeFreeze({
          purge: async (options: PurgeOptions) => {
            const value: unknown = await raw.purge(options);
            if (
              value === null ||
              typeof value !== "object" ||
              Array.isArray(value) ||
              nativeGet(value, "success") !== true ||
              !Number.isSafeInteger(nativeGet(value, "deleted")) ||
              (nativeGet(value, "deleted") as number) < 0
            ) {
              throw new TypeError("CACHE_PROTOCOL_ERROR");
            }
            return value as { success: true; deleted: number };
          },
        }),
        async dispatch(
          origin: () => unknown,
          request: Request,
          ctx: ExecutionContext,
        ): Promise<Response> {
          if (
            !(request instanceof Request) ||
            !["GET", "HEAD"].includes(request.method)
          ) {
            return withCacheStatus(await originResponse(origin), "BYPASS");
          }
          let lookup: CacheLookup;
          try {
            lookup = cacheLookup(
              await raw.match("automatic", undefined, request),
            );
          } catch (error) {
            const code = cacheFailureCode(error);
            if (
              !failOpen ||
              !["CACHE_UNAVAILABLE", "CACHE_RESULT_UNKNOWN"].includes(code)
            ) {
              throw new Error(code);
            }
            return withCacheStatus(await originResponse(origin), "BYPASS");
          }
          if (lookup.response !== undefined) {
            if (lookup.status === "HIT" || lookup.status === "STALE")
              return lookup.response;
            if (lookup.status === "UPDATING") {
              const refresh = originResponse(origin)
                .then(async (response) => {
                  if (!cacheable(request, response)) {
                    await discardResponse(response);
                    return;
                  }
                  return raw.putAutomatic(request, response, lookup);
                })
                .catch(() => undefined);
              ctx.waitUntil(refresh);
              return lookup.response;
            }
            if (lookup.status === "STALE_IF_ERROR") {
              try {
                const response = await originResponse(origin);
                if (response.status >= 500) {
                  await discardResponse(response);
                  return withCacheStatus(lookup.response, "STALE");
                }
                if (cacheable(request, response)) {
                  ctx.waitUntil(
                    raw
                      .putAutomatic(request, response.clone(), lookup)
                      .catch(() => undefined),
                  );
                }
                return withCacheStatus(
                  response,
                  cacheable(request, response) ? "REVALIDATED" : "BYPASS",
                );
              } catch {
                return withCacheStatus(lookup.response, "STALE");
              }
            }
          }
          const response = await originResponse(origin);
          if (!cacheable(request, response) || request.method !== "GET") {
            return withCacheStatus(response, "BYPASS");
          }
          const store = raw
            .putAutomatic(request, response.clone(), lookup)
            .catch(() => undefined);
          ctx.waitUntil(store);
          return withCacheStatus(
            response,
            lookup.status === "EXPIRED" ? "EXPIRED" : "MISS",
          );
        },
      });
    },
  });
}
