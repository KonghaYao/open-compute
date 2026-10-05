import handler from "vinext/server/fetch-handler";

export default {
  fetch(
    request: Request,
    env: Cloudflare.Env,
    context: ExecutionContext,
  ): Promise<Response> {
    return handler.fetch(request, env, context);
  },
};
