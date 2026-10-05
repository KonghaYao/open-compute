import { bindingError } from "../loader/shared.js";

/** Authority operations used by workerd's native Cache HTTP client. */
export interface NativeCacheAuthority {
  match(
    namespace: "default" | "named",
    name: string | undefined,
    request: Request,
  ): Promise<{ status: string; response?: Response }>;
  storeEncoded(
    namespace: "default" | "named",
    name: string | undefined,
    request: Request,
    response: Response,
  ): Promise<void>;
  delete(
    namespace: "default" | "named",
    name: string | undefined,
    request: Request,
  ): Promise<boolean>;
}

/** Parse the pinned Cache PUT payload without buffering the encoded response body.
 * workerd serializes HTTP headers followed by raw bytes, even with Transfer-Encoding: chunked.
 */
export async function nativeCacheResponse(request: Request): Promise<Response> {
  const reader = request.body?.getReader();
  if (!reader) throw bindingError("CACHE_PROTOCOL_ERROR");
  const header = new Uint8Array(64 * 1024);
  let size = 0;
  let remainder: Uint8Array | undefined;
  try {
    readHeader: for (;;) {
      const part = await reader.read();
      if (part.done) throw bindingError("CACHE_PROTOCOL_ERROR");
      for (let index = 0; index < part.value.byteLength; index++) {
        if (size === header.byteLength)
          throw bindingError("CACHE_LIMIT_EXCEEDED");
        header[size++] = part.value[index]!;
        if (
          size >= 4 &&
          header[size - 4] === 13 &&
          header[size - 3] === 10 &&
          header[size - 2] === 13 &&
          header[size - 1] === 10
        ) {
          remainder = part.value.subarray(index + 1);
          break readHeader;
        }
      }
    }
    const lines = new TextDecoder("utf-8", { fatal: true, ignoreBOM: false })
      .decode(header.subarray(0, size - 4))
      .split("\r\n");
    const statusLine = /^HTTP\/1\.1 ([2-5][0-9]{2}) ([^\x00-\x1f\x7f]*)$/.exec(
      lines.shift() ?? "",
    );
    if (!statusLine) throw bindingError("CACHE_PROTOCOL_ERROR");
    const status = Number(statusLine[1]);
    const headers = new Headers();
    for (const line of lines) {
      const match =
        /^([!#$%&'*+.^_`|~0-9A-Za-z-]+):[ \t]*([^\x00-\x08\x0a-\x1f\x7f]*)$/.exec(
          line,
        );
      if (!match) throw bindingError("CACHE_PROTOCOL_ERROR");
      const name = match[1]!.toLowerCase();
      if (name === "content-length" && headers.has(name))
        throw bindingError("CACHE_PROTOCOL_ERROR");
      headers.append(name, match[2]!);
    }
    const rawLength = headers.get("content-length");
    if (rawLength !== null && !/^(?:0|[1-9][0-9]{0,15})$/.test(rawLength))
      throw bindingError("CACHE_PROTOCOL_ERROR");
    const length = rawLength === null ? undefined : Number(rawLength);
    if (length !== undefined && !Number.isSafeInteger(length))
      throw bindingError("CACHE_PROTOCOL_ERROR");
    const transferEncoding = headers.get("transfer-encoding");
    if (
      transferEncoding !== null &&
      (transferEncoding !== "chunked" || length !== undefined)
    )
      throw bindingError("CACHE_PROTOCOL_ERROR");
    // The native serializer has already applied Content-Encoding; no nested HTTP framing remains.
    for (const name of [
      "transfer-encoding",
      "connection",
      "keep-alive",
      "trailer",
      "upgrade",
    ])
      headers.delete(name);
    let received = 0;
    const body = new ReadableStream<Uint8Array>({
      async pull(controller) {
        try {
          let part =
            remainder !== undefined
              ? { done: false, value: remainder }
              : await reader.read();
          remainder = undefined;
          while (!part.done && part.value.byteLength === 0)
            part = await reader.read();
          if (part.done) {
            if (length !== undefined && received !== length)
              throw bindingError("CACHE_PROTOCOL_ERROR");
            controller.close();
            reader.releaseLock();
          } else {
            received += part.value.byteLength;
            if (length !== undefined && received > length)
              throw bindingError("CACHE_PROTOCOL_ERROR");
            if (part.value.byteLength) controller.enqueue(part.value);
          }
        } catch {
          try {
            await reader.cancel();
          } catch {
            /* best effort */
          }
          controller.error(bindingError("CACHE_PROTOCOL_ERROR"));
        }
      },
      cancel(reason) {
        return reader.cancel(reason);
      },
    });
    const noBody = [204, 205, 304].includes(status);
    if (noBody) {
      // Empty-status responses still require complete framing before accepting the write.
      const empty = body.getReader();
      try {
        for (;;) {
          const part = await empty.read();
          if (part.done) break;
          if (part.value.byteLength) throw bindingError("CACHE_PROTOCOL_ERROR");
        }
      } catch (error) {
        try {
          await empty.cancel();
        } catch {
          /* best effort */
        }
        throw error;
      }
    }
    return new Response(noBody ? null : body, {
      status,
      statusText: statusLine[2]!,
      headers,
      encodeBody: "manual",
    });
  } catch (error) {
    try {
      await reader.cancel();
    } catch {
      /* best effort */
    }
    const code: unknown =
      error instanceof Error
        ? Object.getOwnPropertyDescriptor(error, "stableCode")?.value
        : undefined;
    throw bindingError(
      code === "CACHE_LIMIT_EXCEEDED" ? code : "CACHE_PROTOCOL_ERROR",
    );
  }
}

/** Adapt only the native Cache HTTP protocol to the existing persisted authority. */
export async function nativeCacheRequest(
  authority: NativeCacheAuthority,
  request: Request,
): Promise<Response> {
  const encodedName = request.headers.get("cf-cache-namespace");
  let name: string | undefined;
  if (encodedName !== null) {
    try {
      name = decodeURIComponent(encodedName);
    } catch {
      throw bindingError("CACHE_KEY_INVALID");
    }
  }
  const namespace = name === undefined ? "default" : "named";
  const key = new Request(request.url, { headers: request.headers });
  if (request.method === "GET") {
    const lookup = await authority.match(namespace, name, key);
    if (lookup.response) {
      if (lookup.status !== "HIT") {
        try {
          await lookup.response.body?.cancel();
        } catch {
          /* best effort */
        }
        throw bindingError("CACHE_PROTOCOL_ERROR");
      }
      const response = new Response(lookup.response.body, lookup.response);
      response.headers.set("cf-cache-status", "HIT");
      return response;
    }
    if (!["MISS", "EXPIRED"].includes(lookup.status))
      throw bindingError("CACHE_PROTOCOL_ERROR");
    return new Response(null, {
      status: 504,
      headers: { "cf-cache-status": lookup.status },
    });
  }
  if (request.method === "PUT") {
    const response = await nativeCacheResponse(request);
    try {
      await authority.storeEncoded(namespace, name, key, response);
    } catch (error) {
      try {
        await response.body?.cancel();
      } catch {
        /* best effort */
      }
      throw error;
    }
    return new Response(null, { status: 204 });
  }
  if (request.method === "PURGE") {
    const deleted = await authority.delete(namespace, name, key);
    return new Response(null, { status: deleted ? 200 : 404 });
  }
  throw bindingError("CACHE_PROTOCOL_ERROR");
}
