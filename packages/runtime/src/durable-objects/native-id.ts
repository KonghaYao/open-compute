import wrapped from "cloudflare-internal:wrapped-binding";

/** Config-owned factory; never delegated to a tenant environment. */
export default function nativeIdFactory() {
  return Object.freeze({
    create: wrapped.createDurableObjectId.bind(wrapped),
  });
}
