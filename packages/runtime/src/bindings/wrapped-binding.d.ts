/** Internal native base used only by config-owned wrapped-binding extensions. */
declare module "cloudflare-internal:wrapped-binding" {
  class WrappedBinding {
    constructor(fetcher: unknown);
  }
  const binding: {
    WrappedBinding: typeof WrappedBinding;
    createPrivateTransport(transport: unknown): object;
    admitSubrequest(): void;
    createServiceRpcStub(policy: object): object;
    isRpcStub(value: unknown): boolean;
    createDurableObjectId(
      value: string,
      name?: string,
      jurisdiction?: string,
    ): DurableObjectId;
    createDurableObjectNamespace(policy: object): DurableObjectNamespace;
    createDurableObjectStub(
      id: DurableObjectId,
      transport: object,
      policy: object,
    ): DurableObjectStub;
    installQueuePolicy(queue: object, policy: object): void;
    decodeQueueV8(bytes: Uint8Array): unknown;
  };
  export default binding;
}
