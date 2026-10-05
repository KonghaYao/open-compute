"""Images through the unchanged SDK wrapper or raw JavaScript FFI."""

import base64

import js
from pyodide.ffi import to_js
from workers import Blob, Response


def native(value):
    return to_js(value, dict_converter=js.Object.fromEntries, create_pyproxies=False)


async def image_case(env, payload, raw):
    data = base64.b64decode(payload["source"], validate=True)

    def stream(content=data):
        if raw:
            return js.Blob.new(native([memoryview(content)])).stream()
        return Blob([content], content_type="image/png").js_object.stream()

    def options(value):
        return native(value) if raw else value

    if payload.get("operation") == "errors":
        rejected = {}
        for failure in ("input", "options", "decode"):
            try:
                if failure == "input":
                    env.IMAGES.input("invalid-stream")
                elif failure == "options":
                    await env.IMAGES.info(stream(), options({"unsupported": True}))
                else:
                    await env.IMAGES.info(stream(b"invalid-image"))
            except Exception as error:
                rejected[failure] = str(error)
            else:
                rejected[failure] = None
        return {"rejected": rejected}

    info = await env.IMAGES.info(stream())
    if raw:
        info = info.to_py()
    chain = env.IMAGES.input(stream()).transform(options({
        "width": 4, "height": 3, "fit": "pad", "background": "#102030ff",
    }))
    if payload.get("operation") == "draw":
        chain = chain.draw(stream(), options({"left": 1, "top": 1, "opacity": 1, "composite": "over"}))
        chain = chain.transform(options({"rotate": 90}))
    result = await chain.output(options({"format": payload.get("format", "image/png")}))
    content_type = result.contentType()
    if payload.get("image"):
        body = result.image()
        response = js.Response.new(body) if raw else Response(body)
    else:
        response = result.response(options({"headers": {"x-image-test": "custom"}}))
    encoded = (await response.arrayBuffer()).to_bytes() if raw else await response.bytes()
    return {
        "info": info, "contentType": content_type, "status": response.status,
        "header": response.headers.get("x-image-test"),
        "responseContentType": response.headers.get("content-type"),
        "bytes": base64.b64encode(encoded).decode(),
    }
