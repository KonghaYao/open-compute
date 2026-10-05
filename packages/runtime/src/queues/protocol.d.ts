import type { BindingEnv, BindingProps } from "../bindings/protocol.js";
import type { QueueWireCodec } from "./native-adapter.js";

/** Static host-only native decoder; never part of a tenant env or a transferable binding. */
export interface QueueTransportEnv extends BindingEnv {
  QUEUE_WIRE_CODEC: QueueWireCodec;
}

/** Queue authority pinned by a validated version descriptor. */
export interface QueueBindingProps extends BindingProps {
  instanceId: string;
  workerId: string;
  queueId: string;
  queueLifecycleGeneration: number;
  durableObject: boolean;
}
