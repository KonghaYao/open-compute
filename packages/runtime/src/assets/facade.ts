import wrappedBinding from "cloudflare-internal:wrapped-binding";

interface AssetRequestWire {
  readonly url: string;
  readonly method: string;
  readonly headers: readonly (readonly [string, string])[];
}

interface AssetTransport {
  fetchAsset(request: AssetRequestWire): Promise<Response>;
}

/** Tenant-visible Fetcher facade backed by one version-scoped trusted transport. */
export class Fetcher extends wrappedBinding.WrappedBinding {
  readonly #transport: AssetTransport;

  constructor(transport: unknown) {
    super(transport);
    if (
      !transport ||
      typeof transport !== "object" ||
      typeof (transport as Partial<AssetTransport>).fetchAsset !== "function"
    ) {
      throw new TypeError("ASSET_BINDING_UNAVAILABLE");
    }
    this.#transport = transport as AssetTransport;
  }

  fetch(input: RequestInfo | URL, init?: RequestInit): Promise<Response> {
    const request = new Request(input, init);
    return this.#transport.fetchAsset({
      url: request.url,
      method: request.method,
      headers: [...request.headers],
    });
  }
}

/** Construct the config-owned Assets binding from its scoped native Fetcher. */
export default function assetsBinding(env: { fetcher: unknown }): Fetcher {
  return new Fetcher(env.fetcher);
}
