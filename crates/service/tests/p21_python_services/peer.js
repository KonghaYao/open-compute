import { WorkerEntrypoint } from "cloudflare:workers";

const VALUE = {
  message: "µ☁",
  nested: [null, true, { integer: 42, float: 1.25 }],
};

export default class Peer extends WorkerEntrypoint {
  async fetch(request) {
    const path = new URL(request.url).pathname;
    if (path === "/fetch")
      return this.env.TARGET.fetch("https://service.example/echo?v=one&v=two", {
        method: "POST",
        body: "service-body",
        headers: { "X-From": "python" },
      });
    if (path === "/named-fetch")
      return this.env.NAMED.fetch("https://service.example/named");
    if (path === "/rpc")
      return Response.json(await this.env.TARGET.echo(VALUE));
    if (path === "/named-rpc")
      return Response.json({ product: await this.env.NAMED.multiply(6, 7) });
    if (path === "/callback")
      return Response.json(
        await this.env.TARGET.invoke_callback(
          (value) => ({ ...value, message: value.message + "!" }),
          VALUE,
        ),
      );
    if (path === "/failure") {
      try {
        await this.env.TARGET.failure();
      } catch (error) {
        return Response.json({ error: String(error.message) });
      }
      throw new Error("service failure was not propagated");
    }
    if (path === "/uncaught-failure") {
      await this.env.TARGET.secretFailure();
      throw new Error("service failure was not propagated");
    }
    if (path === "/python-identify")
      return Response.json(await this.env.PYTHON.identify());
    if (path === "/python-rpc")
      return Response.json(await this.env.PYTHON.echo(VALUE));
    if (path === "/python-named-rpc")
      return Response.json(await this.env.PY_NAMED.echo(VALUE));
    if (path === "/python-named-fetch")
      return this.env.PY_NAMED.fetch("https://service.example/named");
    return new Response("missing route", { status: 404 });
  }
}
