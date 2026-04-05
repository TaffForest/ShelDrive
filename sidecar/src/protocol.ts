/**
 * JSON-RPC 2.0 protocol for Rust ↔ Node.js sidecar communication over stdin/stdout.
 */

export interface JsonRpcRequest {
  jsonrpc: "2.0";
  id: number;
  method: string;
  params?: Record<string, unknown>;
}

export interface JsonRpcResponse {
  jsonrpc: "2.0";
  id: number;
  result?: unknown;
  error?: JsonRpcError;
}

export interface JsonRpcError {
  code: number;
  message: string;
  data?: unknown;
}

// Method-specific param/result types

export interface PinParams {
  /** Raw content as base64-encoded string */
  content: string;
  /** Optional filename for metadata */
  filename?: string;
  /** Optional MIME type */
  mime_type?: string;
}

export interface PinResult {
  cid: string;
  size_bytes: number;
}

export interface RetrieveParams {
  cid: string;
}

export interface RetrieveResult {
  /** Content as base64-encoded string */
  content: string;
  size_bytes: number;
}

export interface UnpinParams {
  cid: string;
}

export interface UnpinResult {
  success: boolean;
}

export interface ListResult {
  cids: string[];
  count: number;
}

export interface StatusResult {
  connected: boolean;
  network: string;
  node_url: string | null;
}

// Error codes
export const ERR_PARSE = -32700;
export const ERR_INVALID_REQUEST = -32600;
export const ERR_METHOD_NOT_FOUND = -32601;
export const ERR_INVALID_PARAMS = -32602;
export const ERR_INTERNAL = -32603;
export const ERR_NOT_CONNECTED = -32000;
export const ERR_PIN_FAILED = -32001;
export const ERR_RETRIEVE_FAILED = -32002;
export const ERR_UNPIN_FAILED = -32003;
