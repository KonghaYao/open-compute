"""Shared AI Search extension from official entrypoint env or raw env via FFI."""

import js
from pyodide.ffi import JsException, to_js


def native(value):
    return to_js(value, dict_converter=js.Object.fromEntries, create_pyproxies=False)


async def search_case(env, payload):
    operation = payload["operation"]
    name = payload.get("name", "python-runtime-search")
    if operation == "namespace_list":
        return (await env.SEARCH.list(native(payload.get("options", {})))).to_py()
    if operation == "namespace_create":
        instance = await env.SEARCH.create(native({"id": name, "index_method": {"vector": True, "keyword": True}, "score_threshold": 0, "chunk": False}))
        return (await instance.info()).to_py()
    if operation == "namespace_delete":
        await env.SEARCH.delete(name)
        return None
    if operation == "errors":
        rejected = {}
        for failure in ("query", "update", "missing", "isolation"):
            try:
                if failure == "query":
                    await env.SEARCH.get(name).search(native({"query": "alpha", "messages": [{"role": "user", "content": "alpha"}]}))
                elif failure == "update":
                    await env.SEARCH.get(name).update(native({"id": "renamed"}))
                elif failure == "missing":
                    await env.SEARCH.get("missing-instance").info()
                else:
                    await env.ISOLATED_SEARCH.get(name).info()
            except JsException as error:
                rejected[failure] = error.message
            else:
                rejected[failure] = None
        return {"rejected": rejected}
    instance = env.DIRECT_SEARCH if payload.get("target") == "direct" else env.SEARCH.get(name)
    if operation == "info":
        return (await instance.info()).to_py()
    if operation == "stats":
        return (await instance.stats()).to_py()
    if operation == "upload":
        content = payload["content"]
        if payload.get("contentKind") == "blob":
            content = js.Blob.new(native([content]), native({"type": "text/plain"}))
        elif payload.get("contentKind") == "stream":
            content = js.Blob.new(native([content])).stream()
        return (await instance.items.uploadAndPoll(payload["filename"], content, native({"metadata": {"kind": payload["kind"]}, "pollIntervalMs": 50, "timeoutMs": 30000}))).to_py()
    if operation == "items":
        return (await instance.items.list(native(payload.get("options", {})))).to_py()
    if operation == "item_delete":
        await instance.items.delete(payload["itemId"])
        return None
    if operation == "item":
        item = instance.items.get(payload["itemId"])
        download = await item.download()
        body = await js.Response.new(download.body).text()
        return {"info": (await item.info()).to_py(), "logs": (await item.logs()).to_py(), "chunks": (await item.chunks()).to_py(), "download": {"body": body, "filename": download.filename, "contentType": download.contentType, "size": download.size}}
    if operation in ("search", "multi_search"):
        binding = env.SEARCH if operation == "multi_search" else instance
        return (await binding.search(native(payload["request"]))).to_py()
    if operation == "chat":
        return (await instance.chatCompletions(native(payload["request"]))).to_py()
    if operation == "stream":
        stream = await instance.chatCompletions(native(payload["request"]))
        return {"body": await js.Response.new(stream).text()}
    if operation == "jobs":
        return (await instance.jobs.list()).to_py()
    if operation == "job_create":
        return (await instance.jobs.create(native({"description": "Python binding parity"}))).to_py()
    if operation == "job":
        job = instance.jobs.get(payload["jobId"])
        return {"info": (await job.info()).to_py(), "logs": (await job.logs()).to_py()}
    if operation == "job_cancel":
        return (await instance.jobs.get(payload["jobId"]).cancel()).to_py()
    raise ValueError("unsupported test operation")
