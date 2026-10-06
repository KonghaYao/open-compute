import { bindings, defineConfig, exports } from "cf/config";

export default defineConfig({
  worker: {
    name: "stress-demo",
    entrypoint: "./src/index.ts",
    compatibilityDate: "2026-09-08",
    compatibilityFlags: ["nodejs_compat"],
    workersDev: false,
    exports: {
      default: exports.worker(),
      InternalApi: exports.worker(),
      Inventory: exports.durableObject({ storage: "sqlite" }),
      CheckoutFlow: exports.workflow({ name: "stress-demo-checkout-flow" }),
    },
    env: {
      REVISION: bindings.text("p0-stress"),
      TOKEN: bindings.secret(),
      OUTBOUND_URL: bindings.text("http://127.0.0.1:8788/health/live"),
      KV: bindings.kv({ id: "800eea87ed22985e6f490ca8bf2e55d7" }),
      DB: bindings.d1({
        id: "aa52ee31d7e9bb20fbe454d17c681cf5",
        name: "stress-demo-db",
      }),
      BUCKET: bindings.r2({ name: "stress-demo-bucket" }),
      EVENTS: bindings.queue({ name: "stress-demo-events" }),
      INVENTORY: bindings.durableObject({
        worker: "stress-demo",
        exportName: "Inventory",
      }),
      FLOW: bindings.workflow({
        name: "stress-demo-checkout-flow",
        worker: "stress-demo",
        exportName: "CheckoutFlow",
      }),
      SERVICE: bindings.worker({
        worker: "stress-demo",
        exportName: "InternalApi",
      }),
    },
  },
});
