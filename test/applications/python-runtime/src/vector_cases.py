"""The shared Vectorize index through official SDK conversion or raw FFI."""

import js
from pyodide.ffi import to_js


def native(value):
    return to_js(value, dict_converter=js.Object.fromEntries, create_pyproxies=False)


async def vector_case(env, payload, raw):
    def argument(value):
        return native(value) if raw else value

    operation = payload["operation"]
    if operation == "errors":
        rejected = {}
        for failure in ("vector", "topK", "batch"):
            try:
                if failure == "vector":
                    await env.VECTORS.query(argument(["invalid"]))
                elif failure == "topK":
                    await env.VECTORS.query(argument([1, 0, 0]), argument({"topK": 0}))
                else:
                    await env.VECTORS.insert(argument([]))
            except Exception as error:
                rejected[failure] = str(error)
            else:
                rejected[failure] = None
        return {"rejected": rejected}
    if operation == "describe":
        result = await env.VECTORS.describe()
    elif operation in ("query", "queryById"):
        result = await getattr(env.VECTORS, operation)(argument(payload["value"]), argument(payload.get("options", {})))
    elif operation in ("insert", "upsert", "getByIds", "deleteByIds"):
        result = await getattr(env.VECTORS, operation)(argument(payload["value"]))
    else:
        raise ValueError("unsupported test operation")
    return result.to_py() if raw else result
