import { DurableObject, WorkerEntrypoint } from "cloudflare:workers";

const VALUE = { unicode: "µ☁", nested: [null, true, 42, 1.25] };

export default class extends WorkerEntrypoint {
  async fetch(request) {
    const url = new URL(request.url);
    const namespace = this.env.LOCAL;
    const name = url.searchParams.get("name") || "shared";
    if (url.pathname === "/ids") {
      const first = namespace.idFromName(name);
      return Response.json({
        named: first.toString(),
        again: namespace.idFromName(name).toString(),
        unique: namespace.newUniqueId().toString(),
        parsed: namespace.idFromString(first.toString()).toString(),
      });
    }
    if (url.pathname === "/invalid") {
      try {
        namespace.idFromString("invalid-object-id");
      } catch (error) {
        return Response.json({ rejected: Boolean(String(error)) });
      }
      return Response.json({ rejected: false });
    }
    const stub = namespace.getByName(name);
    if (url.pathname === "/socket") return stub.fetch(request);
    if (url.pathname === "/read") return Response.json(await stub.read());
    if (url.pathname === "/increment")
      return Response.json(await stub.increment(1));
    if (url.pathname === "/echo") return Response.json(await stub.echo(VALUE));
    if (url.pathname === "/fetch")
      return stub.fetch(
        new Request("http://object.example/echo?v=one&v=two", {
          method: "POST",
          body: "object-body",
          headers: { "x-from": "caller" },
        }),
      );
    if (url.pathname === "/alarm")
      return Response.json(await stub.arm(Number(url.searchParams.get("at"))));
    if (url.pathname === "/replace") {
      await stub.replace();
      return Response.json({ replaced: false });
    }
    if (url.pathname === "/failure") {
      try {
        await stub.failure();
      } catch (error) {
        return Response.json({ rejected: Boolean(String(error)) });
      }
      return Response.json({ rejected: false });
    }
    return new Response("missing route", { status: 404 });
  }
}

export class Counter extends DurableObject {
  constructor(ctx, env) {
    super(ctx, env);
    this.ctx.blockConcurrencyWhile(async () => {
      this.ctx.storage.sql.exec(
        "CREATE TABLE IF NOT EXISTS state (id INTEGER PRIMARY KEY, value INTEGER NOT NULL)",
      );
      this.ctx.storage.sql.exec("INSERT OR IGNORE INTO state VALUES (1, 0)");
      if ((await this.ctx.storage.get("value")) === undefined)
        await this.ctx.storage.put("value", 0);
      this.boot = ((await this.ctx.storage.get("boots")) || 0) + 1;
      await this.ctx.storage.put("boots", this.boot);
    });
  }
  async read() {
    const row = this.ctx.storage.sql
      .exec("SELECT value FROM state WHERE id=1")
      .one();
    return {
      count: row.value,
      kv: await this.ctx.storage.get("value"),
      alarms: (await this.ctx.storage.get("alarms")) || 0,
      boot: this.boot,
      id: this.ctx.id.toString(),
      name: this.ctx.id.name,
      revision: this.env.REVISION,
      socketMessages: (await this.ctx.storage.get("socketMessages")) || 0,
      socketCloses: (await this.ctx.storage.get("socketCloses")) || 0,
      socketCloseClean:
        (await this.ctx.storage.get("socketCloseClean")) ?? null,
    };
  }
  async increment(amount) {
    this.ctx.storage.sql.exec(
      "UPDATE state SET value=value+? WHERE id=1",
      amount,
    );
    const row = this.ctx.storage.sql
      .exec("SELECT value FROM state WHERE id=1")
      .one();
    await this.ctx.storage.put("value", row.value);
    return this.read();
  }
  echo(value) {
    return value;
  }
  async fetch(request) {
    const url = new URL(request.url);
    if (url.pathname === "/socket") {
      if (request.headers.get("upgrade") !== "websocket")
        return new Response("Expected Upgrade", { status: 426 });
      const [client, server] = Object.values(new WebSocketPair());
      this.ctx.acceptWebSocket(server);
      server.serializeAttachment(
        JSON.stringify({
          id: this.ctx.id.toString(),
          messages: 0,
        }),
      );
      return new Response(null, { status: 101, webSocket: client });
    }
    const state = await this.read();
    return Response.json(
      {
        ...state,
        method: request.method,
        body: await request.text(),
        query: url.searchParams.getAll("v"),
        header: request.headers.get("x-from"),
        host: url.host,
      },
      { status: 201, headers: { "x-actor": "counter" } },
    );
  }
  async webSocketMessage(ws, message) {
    const attachment = JSON.parse(ws.deserializeAttachment());
    if (
      attachment.id !== this.ctx.id.toString() ||
      this.ctx.getWebSockets().length < 1
    )
      throw new Error("invalid socket attachment");
    attachment.messages++;
    ws.serializeAttachment(JSON.stringify(attachment));
    const count = (await this.ctx.storage.get("socketMessages")) || 0;
    await this.ctx.storage.put("socketMessages", count + 1);
    ws.send(message);
  }
  async webSocketClose(ws, code, reason, wasClean) {
    const count = (await this.ctx.storage.get("socketCloses")) || 0;
    await this.ctx.storage.put("socketCloses", count + 1);
    await this.ctx.storage.put("socketCloseClean", wasClean);
    ws.close(code, reason);
  }
  async arm(timestamp) {
    await this.ctx.storage.setAlarm(timestamp);
    return { armed: await this.ctx.storage.getAlarm() };
  }
  async alarm() {
    const count = (await this.ctx.storage.get("alarms")) || 0;
    await this.ctx.storage.put("alarms", count + 1);
  }
  replace() {
    this.ctx.abort("actor replacement");
  }
  failure() {
    throw new Error(this.env.TOKEN);
  }
}
