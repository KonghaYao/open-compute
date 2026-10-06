import type { Env } from "../env";
import { json, jsonError, readJson } from "../lib/json";

interface CheckoutBody {
  orderId?: string;
  mode?: string;
  fanOutN?: number;
}

export async function handleWorkflowCheckout(
  request: Request,
  env: Env,
): Promise<Response> {
  if (request.method !== "POST") {
    return jsonError("method_not_allowed", 405);
  }

  const body = await readJson<CheckoutBody>(request);
  const orderId = body.orderId ?? crypto.randomUUID();
  const mode = body.mode ?? "normal";
  const fanOutN = body.fanOutN ?? 3;

  const instance = await env.FLOW.create({
    id: `checkout-${orderId}`,
    params: { orderId, mode, fanOutN },
  });

  return json(
    {
      stack: "workflow",
      orderId,
      workflowId: instance.id,
      mode,
      fanOutN,
      status: "started",
    },
    202,
  );
}
