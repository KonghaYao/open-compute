import type { CloudflareProductName } from "../components/cloudflare-product-icons";

export type SupportedProductGroup = "Compute" | "Storage and databases" | "AI";

export type SupportedProduct = {
  readonly name: string;
  readonly description: string;
  readonly href: string;
  readonly icon: CloudflareProductName;
  readonly group: SupportedProductGroup;
};

export const supportedProducts: readonly SupportedProduct[] = [
  {
    name: "Workers",
    description: "Deploy and manage serverless applications.",
    href: "/workers",
    icon: "Workers",
    group: "Compute",
  },
  {
    name: "Durable Objects",
    description: "Inspect stateful Worker namespaces.",
    href: "/durable-objects",
    icon: "Durable Objects",
    group: "Compute",
  },
  {
    name: "Queues",
    description: "Connect producers to reliable consumers.",
    href: "/queues",
    icon: "Queues",
    group: "Compute",
  },
  {
    name: "Workflows",
    description: "Run durable multi-step applications.",
    href: "/workflows",
    icon: "Workflows",
    group: "Compute",
  },
  {
    name: "KV",
    description: "Read and write globally addressed key-value data.",
    href: "/kv",
    icon: "KV",
    group: "Storage and databases",
  },
  {
    name: "D1",
    description: "Build with serverless SQL databases.",
    href: "/d1",
    icon: "D1",
    group: "Storage and databases",
  },
  {
    name: "R2",
    description: "Store objects with an S3-compatible API.",
    href: "/r2",
    icon: "R2",
    group: "Storage and databases",
  },
  {
    name: "Vectorize",
    description: "Store and query vector embeddings.",
    href: "/vectorize",
    icon: "Vectorize",
    group: "Storage and databases",
  },
  {
    name: "AI Search",
    description: "Build retrieval-backed AI applications.",
    href: "/ai-search",
    icon: "AI Search",
    group: "AI",
  },
];
