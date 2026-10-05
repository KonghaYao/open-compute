import hashlib
import hmac
import json
from urllib.parse import urlsplit

from workers import Blob, Response, WorkerEntrypoint, fetch
from greeting import MESSAGE


def reply(value):
    return Response(json.dumps(value), headers={"content-type": "application/json"})


class Default(WorkerEntrypoint):
    async def fetch(self, request):
        path = urlsplit(request.url).path
        if path == "/secret-proof":
            proof = hmac.new(
                self.env.TOKEN.encode(), b"python-main-secret-proof", hashlib.sha256,
            ).hexdigest()
            return reply({"proof": proof})
        if path == "/write":
            value = self.env.REVISION
            await self.env.KV.put("value", value)
            await self.env.DB.prepare(
                "CREATE TABLE IF NOT EXISTS state (key TEXT PRIMARY KEY, value TEXT NOT NULL)"
            ).run()
            await self.env.DB.prepare(
                "INSERT INTO state (key, value) VALUES ('value', ?) "
                "ON CONFLICT(key) DO UPDATE SET value=excluded.value"
            ).bind(value).run()
            await self.env.BUCKET.put("value", value)
            return reply({"written": value, "package": MESSAGE})
        if path == "/read":
            kv = await self.env.KV.get("value")
            d1 = await self.env.DB.prepare(
                "SELECT value FROM state WHERE key='value'"
            ).first("value")
            obj = await self.env.BUCKET.get("value")
            r2 = await obj.text() if obj is not None else None
            return reply({"kv": kv, "d1": d1, "r2": r2, "revision": self.env.REVISION})
        if path == "/stream":
            return Response(Blob(["first-", "second"]).js_object.stream())
        if path == "/delete":
            await self.env.KV.delete("value")
            await self.env.DB.prepare("DELETE FROM state WHERE key='value'").run()
            await self.env.BUCKET.delete("value")
            return reply({"deleted": True})
        if path == "/exception":
            raise RuntimeError(self.env.TOKEN)
        if path == "/outbound":
            response = await fetch(self.env.OUTBOUND_URL)
            return Response(await response.text(), status=response.status)
        return reply({"revision": self.env.REVISION, "package": MESSAGE})
