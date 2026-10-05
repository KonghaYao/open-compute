import json
from urllib.parse import parse_qs, urlsplit

import js
from pyodide.ffi import create_proxy, jsnull, to_js
from workers import Response, WorkerEntrypoint, fetch, import_from_javascript
from cache_cases import cache_case
from image_cases import image_case
from ai_cases import ai_case
from vector_cases import vector_case
from artifact_cases import artifact_case
from search_cases import search_case
from http_client_cases import http_client_case


def native(value):
    return to_js(value, dict_converter=js.Object.fromEntries, create_pyproxies=False)


def json_default(value):
    if value is jsnull:
        return None
    raise TypeError("unsupported JSON fixture result")


def reply(value):
    return Response(json.dumps(value, default=json_default), headers={"content-type": "application/json"})


class Default(WorkerEntrypoint):
    async def fetch(self, request):
        url = urlsplit(request.url)
        if url.path == "/http-client":
            return reply(await http_client_case(self.env, parse_qs(url.query)))
        if url.path == "/cache":
            return reply(await cache_case(parse_qs(url.query)))
        raw = parse_qs(url.query).get("caller", ["sdk"])[0] == "ffi"
        env = import_from_javascript("cloudflare:workers").env if raw else self.env
        if url.path == "/images":
            return reply(await image_case(env, await request.json(), raw))
        if url.path == "/ai":
            return reply(await ai_case(env, await request.json(), raw))
        if url.path == "/vectors":
            return reply(await vector_case(env, await request.json(), raw))
        if url.path == "/artifacts":
            return reply(await artifact_case(env, await request.json()))
        if url.path == "/search":
            return reply(await search_case(env, await request.json()))
        value = self.env.REVISION + ("/ffi" if raw else "/sdk")
        key = "runtime/value"
        if url.path == "/assets":
            query = parse_qs(url.query)
            path = query.get("path", ["/message.txt"])[0]
            method = query.get("method", ["GET"])[0]
            headers = {}
            if "etag" in query:
                headers["if-none-match"] = query["etag"][0]
            if "range" in query:
                headers["range"] = query["range"][0]
            response = await env.ASSETS.fetch("https://assets.example" + path, native({"method": method, "headers": headers})) if raw else await env.ASSETS.fetch("https://assets.example" + path, method=method, headers=headers)
            return reply({
                "status": response.status, "body": await response.text(),
                "headers": {name: response.headers.get(name) for name in ("content-type", "content-length", "content-range", "etag")},
            })
        if url.path == "/write":
            await env.KV.put(key, value)
            await env.DB.prepare("CREATE TABLE IF NOT EXISTS runtime_state (id INTEGER PRIMARY KEY, value TEXT NOT NULL)").run()
            await env.DB.prepare("INSERT INTO runtime_state VALUES (1, ?) ON CONFLICT(id) DO UPDATE SET value=excluded.value").bind(value).run()
            await env.BUCKET.put(key, value)
            return reply({"written": value})
        if url.path == "/read":
            obj = await env.BUCKET.get(key)
            return reply({
                "kv": await env.KV.get(key),
                "d1": await env.DB.prepare("SELECT value FROM runtime_state WHERE id=1").first("value"),
                "r2": await obj.text() if obj is not None else None,
            })
        if url.path == "/errors":
            rejected = {}
            for operation in ("kv", "d1", "r2"):
                try:
                    if operation == "kv":
                        options = {"expirationTtl": -1}
                        await env.KV.put("invalid", "value", native(options) if raw else options)
                    elif operation == "d1":
                        await env.DB.prepare("SELECT * FROM table_that_does_not_exist").all()
                    else:
                        options = {"range": {"offset": -1}}
                        await env.BUCKET.get(key, native(options) if raw else options)
                except Exception as error:
                    rejected[operation] = bool(str(error))
                else:
                    rejected[operation] = False
            return reply(rejected)
        if url.path == "/ffi":
            structured = {"unicode": "µ☁", "nested": [None, True, 42, 1.25]}
            converted = native(structured).to_py()
            data = js.Uint8Array.new(native([0, 1, 255]))
            digest = js.Uint8Array.new(await js.crypto.subtle.digest("SHA-256", data))
            callback = create_proxy(lambda item, *unused: item * 2)
            try:
                mapped = js.Array.new(1, 2, 3).map(callback).to_py()
            finally:
                callback.destroy()
            try:
                callback(1)
            except Exception:
                released = True
            else:
                released = False
            return js.Response.new(json.dumps({
                "value": converted, "bytes": list(data.to_bytes()),
                "jsNull": js.JSON.parse("null"),
                "nullDistinctFromUndefined": js.JSON.parse("null") is jsnull and js.Reflect.get(js.Object.new(), "missing") is None,
                "sha256": digest.to_bytes().hex(), "mapped": mapped, "released": released,
            }, default=json_default), native({"status": 201, "headers": {"content-type": "application/json", "x-ffi": "native"}}))
        if url.path == "/stdlib":
            import asyncio
            import base64
            import contextvars
            import hashlib
            import importlib
            import pathlib
            import struct
            import zlib
            from datetime import datetime, timezone
            from decimal import Decimal

            context = contextvars.ContextVar("runtime-context", default="unset")
            context.set("parent")

            async def child(label):
                context.set(label)
                await asyncio.sleep(0)
                return context.get()

            children = await asyncio.gather(child("left"), child("right"))
            unavailable = []
            for module in (
                "curses", "dbm", "ensurepip", "fcntl", "grp", "idlelib", "lib2to3", "msvcrt",
                "pwd", "resource", "syslog", "termios", "tkinter", "turtle", "turtledemo",
                "venv", "winreg", "winsound",
            ):
                try:
                    importlib.import_module(module)
                except ImportError:
                    unavailable.append(module)
            missing_dependency = {}
            for module in ("pty", "tty"):
                try:
                    importlib.import_module(module)
                except ModuleNotFoundError as error:
                    missing_dependency[module] = error.name
            import threading
            import multiprocessing
            try:
                threading.Thread(target=lambda: None).start()
            except RuntimeError:
                thread_rejected = True
            else:
                thread_rejected = False
            data = bytes([0, 1, 255])
            return reply({
                "decimal": str(Decimal("0.1") + Decimal("0.2")),
                "base64": base64.b64encode(data).decode(),
                "sha256": hashlib.sha256(data).hexdigest(),
                "zlib": list(zlib.decompress(zlib.compress(data))),
                "integer": struct.unpack(">I", struct.pack(">I", 42))[0],
                "timestamp": datetime(2026, 1, 1, tzinfo=timezone.utc).timestamp(),
                "path": pathlib.PurePosixPath("a/b").name,
                "contexts": children, "parent": context.get(),
                "excluded": unavailable, "missingDependency": missing_dependency,
                "threadRejected": thread_rejected, "multiprocessingImported": bool(multiprocessing.__name__),
            })
        if url.path == "/env":
            native_env = import_from_javascript("cloudflare:workers").env
            return reply({"keys": sorted(js.Object.keys(native_env).to_py()), "repr": [repr(env.KV), repr(env.DB), repr(env.BUCKET), repr(env.ASSETS), repr(env.IMAGES), repr(env.AI), repr(env.VECTORS), repr(env.ARTIFACTS), repr(env.SEARCH), repr(env.DIRECT_SEARCH), repr(env.ISOLATED_SEARCH)]})
        if url.path == "/background":
            async def write():
                await self.env.KV.put("runtime/background", self.env.REVISION)
            self.ctx.waitUntil(write())
            return reply({"scheduled": True})
        if url.path == "/background-failure":
            async def fail():
                raise RuntimeError(self.env.TOKEN)
            self.ctx.waitUntil(fail())
            return reply({"scheduled": True})
        if url.path == "/background-read":
            return reply(await env.KV.get("runtime/background"))
        if url.path == "/fs-write":
            from pathlib import Path
            path = Path("/tmp/python-runtime-fixture.txt")
            path.write_text(self.env.REVISION)
            return reply({"value": path.read_text()})
        if url.path == "/fs-read":
            from pathlib import Path
            path = Path("/tmp/python-runtime-fixture.txt")
            return reply({"value": path.read_text() if path.exists() else None})
        if url.path == "/log":
            print("python-runtime-log", self.env.TOKEN)
            js.console.log("python-runtime-ffi-log", self.env.TOKEN)
            return reply({"emitted": True})
        if url.path == "/outbound":
            response = await js.fetch(self.env.OUTBOUND_URL, native({
                "method": "POST", "body": "µ☁", "headers": {"x-caller": "ffi"},
            })) if raw else await fetch(self.env.OUTBOUND_URL, method="POST", body="µ☁", headers={"x-caller": "sdk"})
            return Response(await response.text(), status=response.status)
        if url.path == "/tcp":
            destination = urlsplit(self.env.OUTBOUND_URL)
            sockets = import_from_javascript("cloudflare:sockets")
            socket = sockets.connect(native({"hostname": destination.hostname, "port": destination.port}), native({"allowHalfOpen": True}))
            writer = socket.writable.getWriter()
            reader = socket.readable.getReader()
            try:
                await writer.write(js.TextEncoder.new().encode("GET /tcp HTTP/1.1\r\nHost: fixture\r\nConnection: close\r\n\r\n"))
                await writer.close()
                chunks = []
                while True:
                    part = await reader.read()
                    if part.done:
                        break
                    chunks.append(part.value.to_bytes())
                return reply({"response": b"".join(chunks).decode()})
            finally:
                reader.releaseLock()
                writer.releaseLock()
                await socket.close()
                await socket.closed
        if url.path == "/exception":
            raise RuntimeError(self.env.TOKEN)
        return reply({"revision": self.env.REVISION})
