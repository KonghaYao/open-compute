import json
from urllib.parse import parse_qs, urlsplit

from js import WebSocketPair
from workers import DurableObject, Request, Response, WorkerEntrypoint


VALUE = {"unicode": "µ☁", "nested": [None, True, 42, 1.25]}


def reply(value):
    return Response(json.dumps(value), headers={"content-type": "application/json"})


class Default(WorkerEntrypoint):
    async def fetch(self, request):
        url = urlsplit(request.url)
        params = parse_qs(url.query)
        name = params.get("name", ["shared"])[0]
        if url.path == "/ids":
            first = self.env.OBJECTS.idFromName(name)
            again = self.env.OBJECTS.idFromName(name)
            unique = self.env.OBJECTS.newUniqueId()
            parsed = self.env.OBJECTS.idFromString(first.toString())
            return reply({
                "named": first.toString(), "again": again.toString(),
                "unique": unique.toString(), "parsed": parsed.toString(),
            })
        if url.path == "/invalid":
            try:
                self.env.OBJECTS.idFromString("invalid-object-id")
            except Exception as error:
                return reply({"rejected": bool(str(error))})
            return reply({"rejected": False})
        stub = self.env.OBJECTS.getByName(name)
        if url.path == "/socket":
            return await stub.fetch(request)
        if url.path == "/read":
            return reply(await stub.read())
        if url.path == "/increment":
            return reply(await stub.increment(1))
        if url.path == "/echo":
            return reply(await stub.echo(VALUE))
        if url.path == "/fetch":
            return await stub.fetch(Request(
                "http://object.example/echo?v=one&v=two", method="POST",
                body="object-body", headers={"x-from": "caller"},
            ))
        if url.path == "/alarm":
            return reply(await stub.arm(int(params["at"][0])))
        if url.path == "/replace":
            await stub.replace()
            return reply({"replaced": False})
        if url.path == "/failure":
            try:
                await stub.failure()
            except Exception as error:
                return reply({"rejected": bool(str(error))})
            return reply({"rejected": False})
        return Response("missing route", status=404)


class Counter(DurableObject):
    def __init__(self, ctx, env):
        super().__init__(ctx, env)

        @self.ctx.blockConcurrencyWhile
        async def initialize():
            self.ctx.storage.sql.exec(
                "CREATE TABLE IF NOT EXISTS state (id INTEGER PRIMARY KEY, value INTEGER NOT NULL)"
            )
            self.ctx.storage.sql.exec("INSERT OR IGNORE INTO state VALUES (1, 0)")
            if await self.ctx.storage.get("value") is None:
                await self.ctx.storage.put("value", 0)
            self.boot = (await self.ctx.storage.get("boots") or 0) + 1
            await self.ctx.storage.put("boots", self.boot)

    async def read(self):
        row = self.ctx.storage.sql.exec("SELECT value FROM state WHERE id=1").one()
        return {
            "count": row["value"], "kv": await self.ctx.storage.get("value"),
            "alarms": await self.ctx.storage.get("alarms") or 0,
            "boot": self.boot, "id": self.ctx.id.toString(),
            "name": self.ctx.id.name, "revision": self.env.REVISION,
            "socketMessages": await self.ctx.storage.get("socketMessages") or 0,
            "socketCloses": await self.ctx.storage.get("socketCloses") or 0,
            "socketCloseClean": await self.ctx.storage.get("socketCloseClean"),
        }

    async def increment(self, amount):
        self.ctx.storage.sql.exec("UPDATE state SET value=value+? WHERE id=1", amount)
        row = self.ctx.storage.sql.exec("SELECT value FROM state WHERE id=1").one()
        await self.ctx.storage.put("value", row["value"])
        return await self.read()

    def echo(self, value):
        return value

    async def fetch(self, request):
        url = urlsplit(request.url)
        if url.path == "/socket":
            if request.headers.get("upgrade") != "websocket":
                return Response("Expected Upgrade", status=426)
            client, server = WebSocketPair.new()
            self.ctx.acceptWebSocket(server)
            server.serializeAttachment(json.dumps({
                "id": self.ctx.id.toString(), "messages": 0,
            }))
            return Response(None, status=101, web_socket=client)
        value = await self.read()
        value.update({
            "method": request.method, "body": await request.text(),
            "query": parse_qs(url.query)["v"], "header": request.headers.get("x-from"),
            "host": url.netloc,
        })
        return Response(json.dumps(value), status=201, headers={
            "content-type": "application/json", "x-actor": "counter",
        })

    async def webSocketMessage(self, ws, message):
        attachment = json.loads(ws.deserializeAttachment())
        assert attachment["id"] == self.ctx.id.toString()
        assert len(self.ctx.getWebSockets()) >= 1
        attachment["messages"] += 1
        ws.serializeAttachment(json.dumps(attachment))
        count = await self.ctx.storage.get("socketMessages") or 0
        await self.ctx.storage.put("socketMessages", count + 1)
        ws.send(message)

    async def webSocketClose(self, ws, code, reason, was_clean):
        count = await self.ctx.storage.get("socketCloses") or 0
        await self.ctx.storage.put("socketCloses", count + 1)
        await self.ctx.storage.put("socketCloseClean", was_clean)
        ws.close(code, reason)

    async def arm(self, timestamp):
        await self.ctx.storage.setAlarm(timestamp)
        return {"armed": await self.ctx.storage.getAlarm()}

    async def alarm(self):
        count = await self.ctx.storage.get("alarms") or 0
        await self.ctx.storage.put("alarms", count + 1)

    def replace(self):
        self.ctx.abort("actor replacement")

    def failure(self):
        raise RuntimeError(self.env.TOKEN)
