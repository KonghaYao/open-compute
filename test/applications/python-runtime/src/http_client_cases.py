"""Unmodified Pyodide requests/httpx transports on the ordinary deployment path."""

import asyncio
import sys

import httpx
import requests


def error_chain(error):
    pending = [error]
    seen = set()
    names = []
    while pending:
        current = pending.pop()
        if id(current) in seen:
            continue
        seen.add(id(current))
        names.append(type(current).__name__)
        for cause in (current.__cause__, current.__context__, getattr(current, "reason", None), *current.args):
            if isinstance(cause, BaseException):
                pending.append(cause)
    return names


async def http_client_case(env, query):
    client = query["client"][0]
    case = query["case"][0]
    base = query["url"][0]
    timeout = 0.2 if case == "timeout" else 3.0
    path = {"timeout": "slow", "error": "error", "stream": "stream", "cancel": "slow"}.get(case, "echo")
    url = base + path
    marker = str(env.REVISION) + "/" + client
    result = {"client": client, "case": case, "requests": requests.__version__, "httpx": httpx.__version__, "python": sys.version.split()[0]}
    if case == "budget":
        result["completed"] = 0
        for _ in range(3):
            try:
                if client == "requests":
                    with requests.Session() as session:
                        response = session.post(url, data=b"budget", headers={"x-caller": marker}, timeout=3)
                        assert response.status_code == 202
                elif client == "httpx-sync":
                    with httpx.Client(timeout=3) as session:
                        response = session.post(url, content=b"budget", headers={"x-caller": marker})
                        assert response.status_code == 202
                else:
                    assert client == "httpx-async"
                    async with httpx.AsyncClient(timeout=3) as session:
                        response = await session.post(url, content=b"budget", headers={"x-caller": marker})
                        assert response.status_code == 202
                result["completed"] += 1
            except Exception as error:
                result["limited"] = "Too many subrequests" in str(error)
                result["errorType"] = type(error).__name__
                break
        return result
    try:
        if client == "requests":
            with requests.Session() as session:
                if case == "stream":
                    with session.get(url, stream=True, timeout=timeout) as response:
                        result.update(status=response.status_code, body=b"".join(response.iter_content(chunk_size=2)).decode())
                else:
                    response = session.post(url, data=("µ☁/" + marker).encode(), headers={"x-caller": marker}, timeout=timeout)
                    if case == "error":
                        response.raise_for_status()
                    result.update(status=response.status_code, body=response.text, header=response.headers.get("x-fixture"))
        elif client == "httpx-sync":
            with httpx.Client(timeout=timeout) as session:
                if case == "stream":
                    with session.stream("GET", url) as response:
                        result.update(status=response.status_code, body=b"".join(response.iter_bytes(chunk_size=2)).decode())
                else:
                    response = session.post(url, content=("µ☁/" + marker).encode(), headers={"x-caller": marker})
                    if case == "error":
                        response.raise_for_status()
                    result.update(status=response.status_code, body=response.text, header=response.headers.get("x-fixture"))
        else:
            assert client == "httpx-async"
            async with httpx.AsyncClient(timeout=timeout) as session:
                if case == "stream":
                    async with session.stream("GET", url) as response:
                        result.update(status=response.status_code, body=b"".join([chunk async for chunk in response.aiter_bytes(chunk_size=2)]).decode())
                elif case == "cancel":
                    task = asyncio.create_task(session.get(url))
                    # A separate server-side barrier proves the pending HTTP call was admitted.
                    async with httpx.AsyncClient(timeout=3) as observer:
                        await observer.get(base + "pending")
                    task.cancel()
                    try:
                        await task
                    except asyncio.CancelledError:
                        result["cancelled"] = True
                    else:
                        result["cancelled"] = False
                else:
                    response = await session.post(url, content=("µ☁/" + marker).encode(), headers={"x-caller": marker})
                    if case == "error":
                        response.raise_for_status()
                    result.update(status=response.status_code, body=response.text, header=response.headers.get("x-fixture"))
    except (requests.exceptions.Timeout, httpx.TimeoutException) as error:
        result.update(timeout=True, errorType=type(error).__name__)
    except (requests.exceptions.RequestException, httpx.HTTPError) as error:
        result.update(failed=True, errorType=type(error).__name__, errorChain=error_chain(error))
    return result
