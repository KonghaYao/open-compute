import json
from urllib.parse import parse_qs, urlsplit

from workers import Response, WorkerEntrypoint, WorkflowEntrypoint
from workers.workflows import NonRetryableError


def reply(value):
    return Response(json.dumps(value), headers={"content-type": "application/json"})


class Default(WorkerEntrypoint):
    async def fetch(self, request):
        url = urlsplit(request.url)
        query = parse_qs(url.query)
        identifier = query.get("id", ["ordinary"])[0]
        mode = query.get("mode", ["normal"])[0]
        if url.path == "/create":
            instance = await self.env.FLOW.create({
                "id": identifier, "params": {"mode": mode, "value": 42},
            })
            return reply({"id": instance.id})
        if url.path == "/batch":
            instances = await self.env.FLOW.create_batch([
                {"id": identifier + "-a", "params": {"mode": "normal", "value": 42}},
                {"id": identifier + "-b", "params": {"mode": "normal", "value": 42}},
            ])
            return reply({"ids": [instance.id for instance in instances]})
        if url.path == "/invalid":
            try:
                await self.env.FLOW.create_batch([])
            except Exception as error:
                return reply({"rejected": bool(str(error))})
            return reply({"rejected": False})
        if url.path == "/effects":
            return reply(await self.env.KV.get("effects/" + identifier, "json"))
        instance = await self.env.FLOW.get(identifier)
        if url.path == "/status":
            return reply(await instance.status())
        if url.path == "/event":
            await instance.send_event({"type": "continue", "payload": {"unicode": "µ☁"}})
            return reply({"sent": True})
        if url.path == "/pause":
            await instance.pause()
            return reply({"paused": True})
        if url.path == "/resume":
            await instance.resume()
            return reply({"resumed": True})
        if url.path == "/terminate":
            await instance.terminate()
            return reply({"terminated": True})
        return Response("missing route", status=404)


class Flow(WorkflowEntrypoint):
    async def run(self, event, step):
        identifier = event.instanceId
        mode = event.payload["mode"]

        @step.do("prepare", config={
            "retries": {"limit": 2, "delay": 0, "backoff": "constant"},
            "timeout": 1000,
        })
        async def prepare():
            previous = await self.env.KV.get("effects/" + identifier, "json") or {"calls": 0}
            calls = previous["calls"] + 1
            await self.env.KV.put("effects/" + identifier, json.dumps({
                "calls": calls, "revision": self.env.REVISION,
            }))
            if mode in ("fail", "caught"):
                raise NonRetryableError(self.env.TOKEN)
            if mode == "retry" and calls == 1:
                raise RuntimeError(self.env.TOKEN)
            return {"value": event.payload["value"] + 1, "revision": self.env.REVISION}

        try:
            await prepare()
        except NonRetryableError:
            if mode == "caught":
                return {"caught": True}
            raise

        received = None
        if mode == "wait":
            notification = await step.wait_for_event("continue", "continue", timeout="2 minutes")
            received = notification["payload"]
        await step.sleep("checkpoint", 0)

        @step.do("finish")
        async def finish(prepare):
            return {"prepared": prepare, "received": received, "nested": [None, True, 42, 1.25]}

        return await finish()
