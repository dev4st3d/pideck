/**
 * Audited wire contract for bridge/pi-bridge.mjs.
 *
 * The runtime stays dependency-free Node ESM. These types document the additive
 * protocol consumed by Rust; bridge/protocol.schema.json is the machine-readable
 * equivalent. Records are UTF-8 JSONL over inherited stdio only.
 */

export const PROTOCOL_VERSION = 1 as const;

export interface BridgeCapabilities {
  navigateTree: boolean;
  branchSummary: boolean;
  labels: boolean;
  jsonlImport: boolean;
  jsonlExport: boolean;
  sessionList: boolean;
  modelRuntime: boolean;
  providerAuth: boolean;
  modelSettings: boolean;
  resourceInventory: boolean;
  resourceReload: boolean;
  activeToolState: boolean;
  resourceSettings: boolean;
  orchestration: boolean;
  packageMutations: false;
}

export interface RequestRecord {
  version: typeof PROTOCOL_VERSION;
  type: "request";
  id: string;
  command: string;
  params: Record<string, unknown>;
}

export interface CancelRecord {
  version: typeof PROTOCOL_VERSION;
  type: "cancel";
  id: string;
  targetId: string;
}

export interface OrchestrationSnapshotRecord {
  sessionId: string;
  /** Stable for one injected adapter process; changes when its generation resets. */
  producerId?: string;
  generation: number;
  capturedAt: number;
  [key: string]: unknown;
}

export interface ResponseRecord {
  version: typeof PROTOCOL_VERSION;
  type: "response";
  id: string;
  ok: boolean;
  result?: unknown;
  error?: {
    code:
      | "incompatible_protocol"
      | "invalid_json"
      | "invalid_request"
      | "invalid_setting"
      | "operation_failed"
      | "record_too_large"
      | "unsupported_capability"
      | "unsupported_command";
    message: string;
  };
}

export type EventRecord =
  | {
      version: typeof PROTOCOL_VERSION;
      type: "event";
      event: "resource_progress";
      operation: string;
      phase: "start" | "complete";
      message: string;
    }
  | {
      version: typeof PROTOCOL_VERSION;
      type: "event";
      event: "resources_changed";
      generation: number;
    }
  | {
      version: typeof PROTOCOL_VERSION;
      type: "event";
      event: "orchestration_snapshot";
      snapshot: OrchestrationSnapshotRecord;
    }
  | {
      version: typeof PROTOCOL_VERSION;
      type: "event";
      event: "orchestration_disconnected";
    }
  | {
      version: typeof PROTOCOL_VERSION;
      type: "event";
      event:
        | "auth_prompt"
        | "auth_info"
        | "auth_url"
        | "auth_device_code"
        | "auth_progress";
      operationId: number;
      [key: string]: unknown;
    };

export type InboundRecord = RequestRecord | CancelRecord;
export type OutboundRecord = ResponseRecord | EventRecord;
