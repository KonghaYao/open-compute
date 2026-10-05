import datetime
import json
from urllib.parse import parse_qs, urlsplit

from workers import Response, WorkerEntrypoint


def reply(value):
    return Response(json.dumps(value), headers={"content-type": "application/json"})


class Default(WorkerEntrypoint):
    async def fetch(self, request):
        url = urlsplit(request.url)
        params = parse_qs(url.query)
        phase = params.get("phase", ["unused"])[0]
        if url.path == "/send":
            await self.env.EVENTS.send(
                {"label": phase + "-json", "value": [None, True, 42, 1.25, "µ☁"]},
                contentType="json",
            )
            await self.env.EVENTS.sendBatch(
                [
                    {"body": phase + "-text", "contentType": "text"},
                    {"body": (phase + "-bytes").encode(), "contentType": "bytes"},
                ],
                delaySeconds=0,
            )
            await self.env.EVENTS.send(
                {
                    "label": phase + "-v8",
                    "kind": "v8",
                    "value": {
                        "values": [None, True, 42, 1.25, "µ☁"],
                        "when": datetime.datetime.fromtimestamp(1700000000),
                        "unicode": "你" * 50000,
                    },
                },
                contentType="v8",
            )
            return reply({"sent": 4})
        if url.path in ("/retry", "/fail"):
            await self.env.EVENTS.send(
                {"label": phase, "action": url.path[1:]},
                contentType="json",
                delaySeconds=0,
            )
            return reply({"sent": 1})
        if url.path == "/lookup":
            return reply(await self.env.KV.get(params["label"][0], "json"))
        if url.path == "/invalid":
            rejected = []
            for body, options in (
                ("x", {"contentType": "xml"}),
                ("x", {"contentType": "text", "delaySeconds": 86401}),
                (bytes(128001), {"contentType": "bytes"}),
            ):
                try:
                    await self.env.EVENTS.send(body, **options)
                except Exception as error:
                    rejected.append(bool(str(error)))
            return reply({"rejected": rejected})
        return reply({"revision": self.env.REVISION})

    async def queue(self, batch):
        assert batch.queue == "python-queues-events"
        assert batch.metadata.metrics.backlogCount >= 1
        assert batch.metadata.metrics.backlogBytes > 0
        assert isinstance(batch.metadata.metrics.oldestMessageTimestamp, datetime.datetime)
        for message in batch.messages:
            assert isinstance(message.timestamp, datetime.datetime)
            assert isinstance(message.id, str) and message.id
            assert message.attempts >= 1
            body = message.body
            if isinstance(body, dict):
                label = body["label"]
                value = body.get("value")
                action = body.get("action")
                kind = body.get("kind", "json")
                if kind == "v8":
                    assert isinstance(value["when"], datetime.datetime)
                    assert value["unicode"] == "你" * 50000
                    value = {"values": value["values"], "when": value["when"].timestamp(), "unicode": value["unicode"]}
            elif isinstance(body, str):
                label = body
                value = body
                action = None
                kind = "text"
            else:
                data = bytes(memoryview(body))
                label = data.decode()
                value = list(data)
                action = None
                kind = "bytes"
            evidence = {
                "id": message.id,
                "attempts": message.attempts,
                "timestamp": message.timestamp.timestamp(),
                "queue": batch.queue,
                "kind": kind,
                "value": value,
                "revision": self.env.REVISION,
            }
            if action == "retry" and message.attempts == 1:
                evidence["retry"] = True
                await self.env.KV.put(label, json.dumps(evidence))
                message.retry(delaySeconds=0)
                message.ack()  # The first settlement wins.
                continue
            await self.env.KV.put(label, json.dumps(evidence))
            if action == "fail":
                raise RuntimeError(self.env.TOKEN)
            message.ack()
        batch.ackAll()
