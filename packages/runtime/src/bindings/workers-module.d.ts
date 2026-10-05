/** Config-owned event observation on the native internal workers module. */
declare module "cloudflare-internal:workers" {
  const entrypoints: {
    withWaitUntilObserver<T>(
      observer: (promise: Promise<unknown>) => void,
      action: () => T,
    ): T;
  };
  export default entrypoints;
}
