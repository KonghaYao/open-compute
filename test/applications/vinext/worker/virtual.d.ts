declare module "virtual:vinext-worker-entry" {
  const handler: typeof import("vinext/server/app-router-entry").default;
  export default handler;
}
