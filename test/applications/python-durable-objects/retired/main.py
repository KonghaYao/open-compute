import json

from workers import Response, WorkerEntrypoint


class Default(WorkerEntrypoint):
    async def fetch(self, request):
        return Response(json.dumps({"retired": True, "revision": self.env.REVISION}),
                        headers={"content-type": "application/json"})
