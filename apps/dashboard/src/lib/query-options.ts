import { queryOptions, skipToken } from "@tanstack/react-query";
import type { ManagementClient } from "./management-client";

const managementKey =
  <TProduct extends string>(product: TProduct) =>
  (...identity: readonly unknown[]) =>
    ["cloudflare-v4", product, ...identity] as const;

/** One query identity authority shared by list, detail, prefetch, and invalidation. */
export const queryKeys = {
  accounts: managementKey("accounts"),
  aiSearch: managementKey("ai-search"),
  d1: managementKey("d1"),
  durableObjects: managementKey("durable-objects"),
  kv: managementKey("kv"),
  observability: managementKey("observability"),
  platform: managementKey("platform"),
  queues: managementKey("queues"),
  r2: managementKey("r2"),
  service: managementKey("service"),
  vectorize: managementKey("vectorize"),
  workers: managementKey("workers"),
  workflows: managementKey("workflows"),
  product: (productName: string, ...identity: readonly unknown[]) =>
    ["cloudflare-v4", productName, ...identity] as const,
};

export const capabilityQuery = (
  client: ManagementClient | null,
  instanceId: string | null,
) =>
  queryOptions({
    queryKey: queryKeys.platform(instanceId, "capabilities"),
    queryFn:
      client && instanceId
        ? ({ signal }) =>
            client.openCompute.capabilities.getForAccount(instanceId, {
              signal,
            })
        : skipToken,
  });

export const r2BucketQuery = (
  client: ManagementClient | null,
  instanceId: string | null,
  bucketId: string,
) =>
  queryOptions({
    queryKey: queryKeys.r2(instanceId, bucketId),
    queryFn:
      client && instanceId
        ? ({ signal }) =>
            client.r2.buckets.get(
              bucketId,
              { account_id: instanceId },
              { signal },
            )
        : skipToken,
  });

export const workerDeploymentsQuery = (
  client: ManagementClient | null,
  instanceId: string | null,
  workerId: string,
) =>
  queryOptions({
    queryKey: queryKeys.workers(instanceId, workerId, "deployments"),
    queryFn:
      client && instanceId
        ? ({ signal }) =>
            client.workers.scripts.deployments.list(
              workerId,
              { account_id: instanceId },
              { signal },
            )
        : skipToken,
  });

export const d1DatabaseQuery = (
  client: ManagementClient | null,
  instanceId: string | null,
  databaseId: string,
) =>
  queryOptions({
    queryKey: queryKeys.d1(instanceId, databaseId),
    queryFn:
      client && instanceId
        ? ({ signal }) =>
            client.d1.database.get(
              databaseId,
              { account_id: instanceId },
              { signal },
            )
        : skipToken,
  });

export const kvNamespaceQuery = (
  client: ManagementClient | null,
  instanceId: string | null,
  namespaceId: string,
) =>
  queryOptions({
    queryKey: queryKeys.kv(instanceId, namespaceId),
    queryFn:
      client && instanceId
        ? ({ signal }) =>
            client.kv.namespaces.get(
              namespaceId,
              { account_id: instanceId },
              { signal },
            )
        : skipToken,
  });

export const queueQuery = (
  client: ManagementClient | null,
  instanceId: string | null,
  queueId: string,
) =>
  queryOptions({
    queryKey: queryKeys.queues(instanceId, queueId),
    queryFn:
      client && instanceId
        ? ({ signal }) =>
            client.queues.get(queueId, { account_id: instanceId }, { signal })
        : skipToken,
  });

export const workflowQuery = (
  client: ManagementClient | null,
  instanceId: string | null,
  workflowId: string,
) =>
  queryOptions({
    queryKey: queryKeys.workflows(instanceId, workflowId),
    queryFn:
      client && instanceId
        ? ({ signal }) =>
            client.workflows.get(
              workflowId,
              { account_id: instanceId },
              { signal },
            )
        : skipToken,
  });
