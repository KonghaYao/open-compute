import { WorkerEntrypoint, WorkflowEntrypoint } from "cloudflare:workers";
import { NonRetryableError } from "cloudflare:workflows";

export default class extends WorkerEntrypoint {
  async fetch(request) {
    const url = new URL(request.url);
    const id = url.searchParams.get("id") || "ordinary";
    const mode = url.searchParams.get("mode") || "normal";
    if (url.pathname === "/create") {
      const instance = await this.env.FLOW.create({
        id,
        params: { mode, value: 42 },
      });
      return Response.json({ id: instance.id });
    }
    if (url.pathname === "/batch") {
      const instances = await this.env.FLOW.createBatch([
        { id: id + "-a", params: { mode: "normal", value: 42 } },
        { id: id + "-b", params: { mode: "normal", value: 42 } },
      ]);
      return Response.json({ ids: instances.map((instance) => instance.id) });
    }
    if (url.pathname === "/invalid") {
      try {
        await this.env.FLOW.createBatch([]);
      } catch (error) {
        return Response.json({ rejected: Boolean(String(error)) });
      }
      return Response.json({ rejected: false });
    }
    if (url.pathname === "/effects")
      return Response.json(await this.env.KV.get("effects/" + id, "json"));
    const instance = await this.env.FLOW.get(id);
    if (url.pathname === "/status")
      return Response.json(await instance.status());
    if (url.pathname === "/event") {
      await instance.sendEvent({
        type: "continue",
        payload: { unicode: "µ☁" },
      });
      return Response.json({ sent: true });
    }
    if (url.pathname === "/pause") {
      await instance.pause();
      return Response.json({ paused: true });
    }
    if (url.pathname === "/resume") {
      await instance.resume();
      return Response.json({ resumed: true });
    }
    if (url.pathname === "/terminate") {
      await instance.terminate();
      return Response.json({ terminated: true });
    }
    return new Response("missing route", { status: 404 });
  }
}

export class Flow extends WorkflowEntrypoint {
  async run(event, step) {
    const mode = event.payload.mode;
    let prepared;
    try {
      prepared = await step.do(
        "prepare",
        {
          retries: { limit: 2, delay: 0, backoff: "constant" },
          timeout: 1000,
        },
        async () => {
          const key = "effects/" + event.instanceId;
          const previous = (await this.env.KV.get(key, "json")) || { calls: 0 };
          const calls = previous.calls + 1;
          await this.env.KV.put(
            key,
            JSON.stringify({ calls, revision: this.env.REVISION }),
          );
          if (mode === "fail" || mode === "caught")
            throw new NonRetryableError(this.env.TOKEN);
          if (mode === "retry" && calls === 1) throw new Error(this.env.TOKEN);
          return {
            value: event.payload.value + 1,
            revision: this.env.REVISION,
          };
        },
      );
    } catch (error) {
      if (error instanceof NonRetryableError && mode === "caught")
        return { caught: true };
      throw error;
    }
    let received = null;
    if (mode === "wait") {
      const notification = await step.waitForEvent("continue", {
        type: "continue",
        timeout: "2 minutes",
      });
      received = notification.payload;
    }
    await step.sleep("checkpoint", 0);
    return step.do("finish", async () => ({
      prepared,
      received,
      nested: [null, true, 42, 1.25],
    }));
  }
}
