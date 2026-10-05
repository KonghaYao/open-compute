"""Native Cache API through official SDK Request/Response conversion or raw FFI."""

import js
from pyodide.ffi import to_js
from workers import Request, Response

KEY = "https://python-runtime-cache.invalid/value"
HEADER_NAMES = ("content-type", "cache-control", "etag", "content-length", "content-range")


def options(value):
    return to_js(value, dict_converter=js.Object.fromEntries, create_pyproxies=False)


async def cache_case(query):
    raw = query.get("caller", ["sdk"])[0] == "ffi"
    name = query.get("namespace", ["default"])[0]
    cache = js.caches.default if name == "default" else await js.caches.open("runtime-cache-named")
    operation = query.get("op", ["match"])[0]
    value = query.get("value", ["cache-body"])[0]
    method = query.get("method", ["GET"])[0]
    ignore_method = query.get("ignore_method", ["false"])[0] == "true"
    headers = {}
    if "range" in query:
        headers["range"] = query["range"][0]
    if "etag" in query:
        headers["if-none-match"] = query["etag"][0]

    def request(verb, request_headers):
        return js.Request.new(KEY, options({"method": verb, "headers": request_headers})) if raw else Request(KEY, method=verb, headers=request_headers).js_object

    def response(status=200, extra=None):
        response_headers = {
            "content-type": "text/plain", "cache-control": "public, max-age=3600",
            "etag": '"' + value + '"', "content-length": str(len(value.encode())),
        }
        response_headers.update(extra or {})
        return js.Response.new(value, options({"status": status, "headers": response_headers})) if raw else Response(value, status=status, headers=response_headers).js_object

    if operation == "put":
        await cache.put(request("GET", {}), response())
        return {"stored": True}
    if operation == "delete":
        return {"deleted": bool(await cache.delete(request(method, headers), options({"ignoreMethod": ignore_method})))}
    if operation == "errors":
        rejected = {}
        for failure in ("method", "partial", "vary"):
            try:
                await cache.put(
                    request("POST" if failure == "method" else "GET", {}),
                    response(206 if failure == "partial" else 200, {"vary": "*"} if failure == "vary" else None),
                )
            except Exception as error:
                rejected[failure] = bool(str(error))
            else:
                rejected[failure] = False
        return rejected
    if operation != "match":
        raise ValueError("unknown cache fixture operation")
    cached = await cache.match(request(method, headers), options({"ignoreMethod": ignore_method}))
    if cached is None:
        return {"found": False}
    body = await cached.text() if raw else await Response(cached).text()
    return {
        "found": True, "status": cached.status, "body": body,
        "headers": {name: cached.headers.get(name) for name in HEADER_NAMES},
    }
