import { WorkerEntrypoint } from "cloudflare:workers";

export default class extends WorkerEntrypoint {
  async fetch(request) {
    const url = new URL(request.url);
    const phase = url.searchParams.get("phase") || "unused";
    if (url.pathname === "/send") {
      await this.env.EVENTS.send(
        {
          label: `${phase}-json`,
          value: [null, true, 42, 1.25, "µ☁"],
        },
        { contentType: "json" },
      );
      await this.env.EVENTS.sendBatch(
        [
          { body: `${phase}-text`, contentType: "text" },
          {
            body: new TextEncoder().encode(`${phase}-bytes`),
            contentType: "bytes",
          },
        ],
        { delaySeconds: 0 },
      );
      await this.env.EVENTS.send(
        {
          label: `${phase}-v8`,
          kind: "v8",
          value: {
            values: [null, true, 42, 1.25, "µ☁"],
            when: new Date(1700000000000),
            unicode: "你".repeat(50000),
          },
        },
        { contentType: "v8" },
      );
      return Response.json({ sent: 4 });
    }
    if (url.pathname === "/lookup")
      return Response.json(
        await this.env.KV.get(url.searchParams.get("label"), "json"),
      );
    if (url.pathname === "/invalid") {
      const rejected = [];
      for (const [body, options] of [
        ["x", { contentType: "xml" }],
        ["x", { contentType: "text", delaySeconds: 86401 }],
        [new Uint8Array(128001), { contentType: "bytes" }],
      ]) {
        try {
          await this.env.EVENTS.send(body, options);
        } catch (error) {
          rejected.push(Boolean(String(error)));
        }
      }
      return Response.json({ rejected });
    }
    return Response.json({ revision: this.env.REVISION });
  }

  async queue(batch) {
    if (
      batch.queue !== "python-queues-events" ||
      batch.metadata.metrics.backlogCount < 1 ||
      batch.metadata.metrics.backlogBytes < 1 ||
      !(batch.metadata.metrics.oldestMessageTimestamp instanceof Date)
    )
      throw new Error("invalid queue metadata");
    for (const message of batch.messages) {
      if (
        !(message.timestamp instanceof Date) ||
        !message.id ||
        message.attempts < 1
      )
        throw new Error("invalid message metadata");
      const body = message.body;
      let kind =
        typeof body === "string"
          ? "text"
          : body instanceof Uint8Array
            ? "bytes"
            : "json";
      const bytes = kind === "bytes" ? body : undefined;
      const label =
        kind === "json"
          ? body.label
          : kind === "text"
            ? body
            : new TextDecoder().decode(bytes);
      let value =
        kind === "json"
          ? (body.value ?? null)
          : kind === "text"
            ? body
            : [...bytes];
      if (body?.kind === "v8") {
        if (!(value.when instanceof Date))
          throw new Error("lost structured Date");
        kind = "v8";
        if (value.unicode !== "你".repeat(50000))
          throw new Error("lost structured Unicode");
        value = {
          values: value.values,
          when: value.when.getTime() / 1000,
          unicode: value.unicode,
        };
      }
      const evidence = {
        id: message.id,
        attempts: message.attempts,
        timestamp: message.timestamp.getTime() / 1000,
        queue: batch.queue,
        kind,
        value,
        revision: this.env.REVISION,
      };
      if (body?.action === "retry" && message.attempts === 1) {
        evidence.retry = true;
        await this.env.KV.put(label, JSON.stringify(evidence));
        message.retry({ delaySeconds: 0 });
        message.ack();
        continue;
      }
      await this.env.KV.put(label, JSON.stringify(evidence));
      if (body?.action === "fail") throw new Error(this.env.TOKEN);
      message.ack();
    }
    batch.ackAll();
  }
}
