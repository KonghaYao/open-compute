import json
from urllib.parse import urlsplit

from workers import Response, WorkerEntrypoint, python_from_rpc, python_to_rpc

VALUE = {"message": "µ☁", "nested": [None, True, {"integer": 42, "float": 1.25}]}


def reply(value):
    return Response(json.dumps(value), headers={"content-type": "application/json"})


class Default(WorkerEntrypoint):
    async def fetch(self, request):
        path = urlsplit(request.url).path
        if path == "/fetch":
            response = await self.env.TARGET.fetch(
                "https://service.example/echo?v=one&v=two",
                method="POST",
                body="service-body",
                headers={"X-From": "python"},
            )
            return Response(await response.text(), status=response.status)
        if path == "/named-fetch":
            response = await self.env.NAMED.fetch("https://service.example/named")
            return Response(await response.text(), status=response.status)
        if path == "/rpc":
            return reply(await self.env.TARGET.echo(VALUE))
        if path == "/named-rpc":
            return reply({"product": await self.env.NAMED.multiply(6, 7)})
        if path == "/callback":
            def callback(value):
                value = python_from_rpc(value)
                value["message"] += "!"
                return python_to_rpc(value)
            return reply(await self.env.TARGET.invoke_callback(callback, VALUE))
        if path == "/failure":
            try:
                await self.env.TARGET.failure()
            except Exception as error:
                return reply({"error": str(error)})
            raise AssertionError("service failure was not propagated")
        return reply(self.identify())

    def identify(self):
        return {"entrypoint": "default", "revision": self.env.REVISION}

    def echo(self, value):
        return {"value": value, "revision": self.env.REVISION}


class NamedApi(WorkerEntrypoint):
    async def fetch(self, request):
        return reply({"entrypoint": "named", "revision": self.env.REVISION})

    def echo(self, value):
        return {"value": value, "revision": self.env.REVISION}
