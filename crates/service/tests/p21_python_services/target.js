import { WorkerEntrypoint } from "cloudflare:workers";

export default class Target extends WorkerEntrypoint {
  async fetch(request) {
    const url = new URL(request.url);
    return Response.json({
      method: request.method,
      body: await request.text(),
      query: url.searchParams.getAll("v"),
      header: request.headers.get("X-From"),
      host: url.hostname,
      revision: this.env.REVISION,
    });
  }
  echo(value) {
    return { value, revision: this.env.REVISION };
  }
  invoke_callback(callback, value) {
    return callback(value);
  }
  failure() {
    throw new Error("python-services-business-failure");
  }
  secretFailure() {
    throw new Error(this.env.TOKEN);
  }
}

export class NamedApi extends WorkerEntrypoint {
  fetch() {
    return Response.json({ entrypoint: "named", revision: this.env.REVISION });
  }
  multiply(left, right) {
    return left * right;
  }
}
