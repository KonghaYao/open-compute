/** Bound a private body stream and cancel the upstream source on overflow. */
export function bindingBody(
  input: ReadableStream<Uint8Array> | null,
  maximum: number,
  errorCode: string,
) {
  let size = 0;
  return input?.pipeThrough(
    new TransformStream<Uint8Array, Uint8Array>({
      transform(chunk, controller) {
        size += chunk.byteLength;
        if (size > maximum) throw new TypeError(errorCode);
        controller.enqueue(chunk);
      },
    }),
  );
}

/** Read one bounded JSON request and cancel the upstream stream on overflow. */
export async function bindingJson(
  request: Request,
  maximum: number,
  errorCode: string,
): Promise<unknown> {
  const body = bindingBody(request.body, maximum, errorCode);
  try {
    return (await new Response(body).json()) as unknown;
  } catch (error) {
    if (error instanceof SyntaxError) throw new TypeError(errorCode);
    throw error;
  }
}
