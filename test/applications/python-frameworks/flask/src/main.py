from workers import WorkerEntrypoint, wsgi
from flask import Flask, request, jsonify, Response, render_template, stream_with_context
app = Flask(__name__)
app.config.update(DEBUG=False, TESTING=False, SECRET_KEY="framework-fixture-key")
closed = 0

@app.route("/echo", methods=["GET", "POST"])
def echo():
    response = jsonify(method=request.method, body=request.get_data().decode(), query=request.args.getlist("v"), revision=request.environ["workers.env"].REVISION)
    response.status_code = 201
    response.headers["X-App"] = "flask"
    return response

@app.get("/stream")
def stream():
    @stream_with_context
    def body():
        global closed
        try:
            yield "flask:"
            yield request.args["value"]
        finally:
            closed += 1
    return Response(body(), content_type="text/plain")

@app.get("/state")
def state():
    return jsonify(closed=closed)

@app.get("/template")
def template():
    return render_template("page.html", value=request.args["value"])

@app.get("/fail")
def fail():
    raise RuntimeError(request.environ["workers.env"].TOKEN)

class Default(WorkerEntrypoint):
    async def fetch(self, request):
        return await wsgi.fetch(app, request, self.env, self.ctx)
