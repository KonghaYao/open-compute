from django.http import JsonResponse, StreamingHttpResponse
from django.urls import path
closed = 0

def echo(request):
    response = JsonResponse({"method": request.method, "body": request.body.decode(), "query": request.GET.getlist("v"), "revision": request.META["workers.env"].REVISION}, status=201)
    response["X-App"] = "django"
    return response

def stream(request):
    def body():
        global closed
        try:
            yield b"django:"
            yield request.GET["value"].encode()
        finally:
            closed += 1
    return StreamingHttpResponse(body(), content_type="text/plain")

def state(request):
    return JsonResponse({"closed": closed})

def fail(request):
    raise RuntimeError(request.META["workers.env"].TOKEN)

urlpatterns = [path("echo", echo), path("stream", stream), path("state", state), path("fail", fail)]
