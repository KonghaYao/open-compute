"""The declared Markdown Conversion subset through SDK or JavaScript FFI."""

import js
from pyodide.ffi import to_js
from workers import Blob


def native(value):
    return to_js(value, dict_converter=js.Object.fromEntries, create_pyproxies=False)


async def ai_case(env, payload, raw):
    def document(name="sample.md", content="# Native Python\n\nmarkdown bridge", mime="text/markdown"):
        blob = js.Blob.new(native([content]), native({"type": mime})) if raw else Blob([content], content_type=mime)
        return {"name": name, "blob": blob}

    def argument(value):
        return native(value) if raw else value

    service = env.AI.toMarkdown()
    operation = payload.get("operation", "single")
    if operation == "supported":
        result = await service.supported()
    elif operation == "errors":
        rejected = {}
        for failure in ("document", "options", "inference"):
            try:
                if failure == "document":
                    await env.AI.toMarkdown(argument(document(name="../sample.md")))
                elif failure == "options":
                    await env.AI.toMarkdown(argument(document()), argument({"gateway": {}}))
                else:
                    await env.AI.run("@cf/unsupported", argument({"prompt": "example"}))
            except Exception as error:
                rejected[failure] = str(error)
            else:
                rejected[failure] = None
        return {"rejected": rejected}
    elif operation == "batch":
        result = await env.AI.toMarkdown(argument([
            document(), document("bad.png", "invalid image", "image/png"),
        ]))
    elif operation == "transform":
        result = await service.transform(argument(document("sample.txt", "handle transform", "text/plain")))
    elif operation == "text":
        result = await service.transform(argument(document()), argument({"conversionOptions": {"output": {"format": "text"}}}))
    else:
        result = await env.AI.toMarkdown(argument(document()))
    if raw:
        result = result.to_py()
    return {"result": result, "aiGatewayLogId": env.AI.aiGatewayLogId}
