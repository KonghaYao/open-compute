// Developer-only capture of cf's real Static Assets session and multipart upload.
import { readFile, realpath, writeFile } from "node:fs/promises";
import type { IncomingMessage, ServerResponse } from "node:http";
import { isAbsolute, join, relative } from "node:path";
import { sha256 } from "../../../scripts/workerd-archive.ts";

const header = Buffer.from('{"alg":"HS256","typ":"JWT"}').toString("base64url");
const payload = Buffer.from(
  '{"exp":4102444800,"sub":"python-assets-capture-only"}',
).toString("base64url");
const uploadToken = `${header}.${payload}.capture-upload-only`;
export const completeAssetToken = `${header}.${payload}.capture-complete-only`;
const limit = 8 * 1024 * 1024;

async function body(request: IncomingMessage): Promise<Buffer> {
  const chunks: Buffer[] = [];
  let size = 0;
  for await (const chunk of request) {
    if (!Buffer.isBuffer(chunk)) throw new Error("invalid asset upload chunk");
    size += chunk.byteLength;
    if (size > limit) throw new Error("asset capture body too large");
    chunks.push(chunk);
  }
  return Buffer.concat(chunks);
}

function success(response: ServerResponse, result: unknown) {
  response.writeHead(200, { "content-type": "application/json" });
  response.end(
    JSON.stringify({ success: true, errors: [], messages: [], result }),
  );
}

/** Keep the uploader protocol evidence separate from the original Worker multipart. */
export class PythonAssetCapture {
  readonly #entries = new Map<
    string,
    { paths: string[]; bytes: Buffer; uploaded: boolean }
  >();
  #session = false;
  #complete = false;

  readonly directory: string;
  readonly destination: string;
  readonly sessionPath: string;
  readonly uploadPath: string;
  readonly accountToken: string;

  constructor(
    directory: string,
    destination: string,
    sessionPath: string,
    uploadPath: string,
    accountToken: string,
  ) {
    this.directory = directory;
    this.destination = destination;
    this.sessionPath = sessionPath;
    this.uploadPath = uploadPath;
    this.accountToken = accountToken;
  }

  async handle(
    request: IncomingMessage,
    response: ServerResponse,
  ): Promise<boolean> {
    const url = new URL(request.url ?? "/", "http://127.0.0.1");
    if (url.pathname !== this.sessionPath && url.pathname !== this.uploadPath)
      return false;
    if (request.method !== "POST")
      throw new Error("unexpected asset capture method");
    if (url.pathname === this.sessionPath) {
      if (
        this.#session ||
        url.search ||
        request.headers.authorization !== `Bearer ${this.accountToken}`
      )
        throw new Error("invalid asset session capture");
      const bytes = await body(request);
      const parsed: unknown = JSON.parse(bytes.toString("utf8"));
      if (
        typeof parsed !== "object" ||
        parsed === null ||
        !("manifest" in parsed) ||
        typeof parsed.manifest !== "object" ||
        parsed.manifest === null ||
        Array.isArray(parsed.manifest)
      )
        throw new Error("invalid asset manifest capture");
      const root = await realpath(this.directory);
      const manifest = parsed.manifest as Record<string, unknown>;
      for (const [path, value] of Object.entries(manifest)) {
        if (
          !path.startsWith("/") ||
          path
            .slice(1)
            .split("/")
            .some((part) => !part || part === "." || part === "..") ||
          path.includes("\\") ||
          typeof value !== "object" ||
          value === null ||
          !("hash" in value) ||
          typeof value.hash !== "string" ||
          !/^[0-9a-f]{32}$/.test(value.hash) ||
          !("size" in value) ||
          typeof value.size !== "number" ||
          !Number.isSafeInteger(value.size) ||
          value.size < 0 ||
          value.size > limit
        )
          throw new Error("invalid asset manifest entry");
        const file = await realpath(join(root, path.slice(1)));
        const scope = relative(root, file);
        if (
          !scope ||
          scope === ".." ||
          scope.startsWith("../") ||
          isAbsolute(scope)
        )
          throw new Error("asset file escaped capture root");
        const source = await readFile(file);
        if (source.byteLength !== value.size)
          throw new Error("asset file size differs from manifest");
        const existing = this.#entries.get(value.hash);
        if (existing) {
          if (!existing.bytes.equals(source))
            throw new Error("asset hash collision in capture");
          existing.paths.push(path);
        } else {
          this.#entries.set(value.hash, {
            paths: [path],
            bytes: source,
            uploaded: false,
          });
        }
      }
      if (!this.#entries.size) throw new Error("empty asset capture manifest");
      await writeFile(join(this.destination, "assets-session.json"), bytes, {
        flag: "wx",
      });
      this.#session = true;
      success(response, {
        buckets: [[...this.#entries.keys()]],
        jwt: uploadToken,
      });
      return true;
    }
    if (
      !this.#session ||
      this.#complete ||
      url.search !== "?base64=true" ||
      request.headers.authorization !== `Bearer ${uploadToken}`
    )
      throw new Error("invalid asset batch capture");
    const bytes = await body(request);
    const contentType = request.headers["content-type"] ?? "";
    const form = await new Request("http://127.0.0.1/assets", {
      method: "POST",
      headers: { "content-type": contentType },
      body: bytes,
    }).formData();
    const entries: {
      paths: string[];
      hash: string;
      mime: string;
      size: number;
      sha256: string;
    }[] = [];
    for (const [hash, part] of form) {
      const entry = this.#entries.get(hash);
      if (!entry || entry.uploaded || typeof part === "string")
        throw new Error("unexpected asset upload part");
      const encoded = await part.text();
      const decoded = Buffer.from(encoded, "base64");
      if (
        decoded.toString("base64") !== encoded ||
        !decoded.equals(entry.bytes)
      )
        throw new Error("asset upload differs from builder bytes");
      entry.uploaded = true;
      entries.push({
        paths: entry.paths,
        hash,
        mime: part.type,
        size: decoded.byteLength,
        sha256: sha256(decoded),
      });
    }
    if (entries.length !== this.#entries.size)
      throw new Error("incomplete asset batch capture");
    await writeFile(join(this.destination, "assets-upload.multipart"), bytes, {
      flag: "wx",
    });
    await writeFile(
      join(this.destination, "assets-upload.json"),
      JSON.stringify({ contentType, sha256: sha256(bytes), entries }) + "\n",
      { flag: "wx" },
    );
    this.#complete = true;
    success(response, { jwt: completeAssetToken });
    return true;
  }

  assertComplete(metadata: Record<string, unknown>) {
    if (
      !this.#complete ||
      typeof metadata.assets !== "object" ||
      metadata.assets === null ||
      !("jwt" in metadata.assets) ||
      metadata.assets.jwt !== completeAssetToken
    )
      throw new Error("Worker upload did not redeem completed asset capture");
  }
}
