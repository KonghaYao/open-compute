from workers import WorkerEntrypoint, asgi
from fastapi import FastAPI, Request
from pydantic import BaseModel, Field
from starlette.responses import StreamingResponse

app = FastAPI()

class Input(BaseModel):
    count: int = Field(ge=1)
    label: str

@app.post("/echo", status_code=201)
async def echo(value: Input, request: Request):
    return {"count": value.count, "label": value.label, "revision": request.scope["env"].REVISION}

@app.get("/sync")
def sync_handler():
    return {"sync": True}

@app.get("/stream")
async def stream(request: Request):
    async def body():
        yield b"fastapi:"
        yield request.query_params["value"].encode()
    return StreamingResponse(body(), media_type="text/plain")

@app.get("/fail")
async def fail(request: Request):
    raise RuntimeError(request.scope["env"].TOKEN)

class Default(WorkerEntrypoint):
    async def fetch(self, request):
        return await asgi.fetch(app, request, self.env, self.ctx)
