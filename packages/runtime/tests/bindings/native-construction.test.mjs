import assert from "node:assert/strict";
import test from "node:test";
import {
  compileRuntime,
  importRuntime,
  moduleUrl,
} from "../compiled-runtime.mjs";

const products = await Promise.all(
  ["images", "ai", "artifacts", "assets", "vectorize"].map(async (name) => ({
    name,
    factory: (await importRuntime(`${name}/facade.ts`)).default,
    raw: {
      input() {},
      info() {},
      transform() {},
      supported() {},
      call() {},
      mutate() {},
      fetchAsset() {},
    },
  })),
);

const validation = moduleUrl(await compileRuntime("ai-search/validation.ts"));
const responses = moduleUrl(
  await compileRuntime("ai-search/responses.ts", {
    "./validation.js": validation,
  }),
);
const facade = moduleUrl(
  await compileRuntime("ai-search/facade.ts", {
    "./responses.js": responses,
    "./validation.js": validation,
  }),
);
for (const kind of ["namespace", "instance"]) {
  products.push({
    name: `ai-search-${kind}`,
    factory: (
      await importRuntime(`ai-search/${kind}-binding.ts`, {
        "./facade.js": facade,
      })
    ).default,
    raw: { call() {}, stream() {}, upload() {}, download() {} },
  });
}

test("native wrapped construction never passes a raw Fetcher to tenant globals", () => {
  const transports = new Set(products.map(({ raw }) => raw));
  const originalGet = Reflect.get;
  const originalApply = Reflect.apply;
  const originalIsArray = Array.isArray;
  const privateValue = (value) => {
    assert.equal(
      transports.has(value),
      false,
      "raw transport reached a tenant global",
    );
  };
  Reflect.get = (target, ...args) => {
    privateValue(target);
    return originalGet(target, ...args);
  };
  Reflect.apply = (fn, receiver, args) => {
    privateValue(receiver);
    return originalApply(fn, receiver, args);
  };
  Array.isArray = (value) => {
    privateValue(value);
    return originalIsArray(value);
  };
  try {
    for (const { name, factory, raw } of products) {
      const binding = factory({ fetcher: raw });
      assert.deepEqual(
        Object.keys(binding),
        name === "ai" ? ["aiGatewayLogId"] : [],
      );
      assert.deepEqual(Object.getOwnPropertySymbols(binding), []);
      for (const fetcher of [null, {}, "invalid"])
        assert.throws(() => factory({ fetcher }));
    }
  } finally {
    Reflect.get = originalGet;
    Reflect.apply = originalApply;
    Array.isArray = originalIsArray;
  }
});
