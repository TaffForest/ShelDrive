/**
 * ShelDrive Sidecar — JSON-RPC server over stdin/stdout.
 *
 * The Rust Tauri process spawns this as a child process and communicates
 * via newline-delimited JSON-RPC 2.0 messages.
 */

import * as readline from "node:readline";
import {
  type JsonRpcRequest,
  type JsonRpcResponse,
  ERR_METHOD_NOT_FOUND,
  ERR_INTERNAL,
  ERR_PARSE,
} from "./protocol.js";
import * as shelby from "./shelby-client.js";

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

const config = {
  network: process.env.SHELBY_NETWORK ?? "SHELBYNET",
  rpcBaseUrl: process.env.SHELBY_RPC_URL ?? null,
  apiKey: process.env.SHELBY_API_KEY ?? null,
  privateKey: process.env.SHELBY_PRIVATE_KEY ?? null,
};

// ---------------------------------------------------------------------------
// Response helpers
// ---------------------------------------------------------------------------

function sendResponse(res: JsonRpcResponse): void {
  process.stdout.write(JSON.stringify(res) + "\n");
}

function successResponse(id: number, result: unknown): JsonRpcResponse {
  return { jsonrpc: "2.0", id, result };
}

function errorResponse(
  id: number,
  code: number,
  message: string,
  data?: unknown
): JsonRpcResponse {
  return { jsonrpc: "2.0", id, error: { code, message, data } };
}

// ---------------------------------------------------------------------------
// Request handler
// ---------------------------------------------------------------------------

async function handleRequest(req: JsonRpcRequest): Promise<void> {
  const { id, method, params } = req;

  try {
    switch (method) {
      case "shelby.ping": {
        sendResponse(successResponse(id, { pong: true, timestamp: Date.now() }));
        break;
      }

      case "shelby.status": {
        const status = shelby.getStatus(config);
        sendResponse(successResponse(id, status));
        break;
      }

      case "shelby.pin": {
        if (!params || typeof params.content !== "string") {
          sendResponse(
            errorResponse(id, ERR_INTERNAL, "Missing content parameter")
          );
          break;
        }
        const pinResult = await shelby.pin({
          content: params.content as string,
          filename: params.filename as string | undefined,
          mime_type: params.mime_type as string | undefined,
        });
        sendResponse(successResponse(id, pinResult));
        break;
      }

      case "shelby.retrieve": {
        if (!params || typeof params.cid !== "string") {
          sendResponse(
            errorResponse(id, ERR_INTERNAL, "Missing cid parameter")
          );
          break;
        }
        const retrieveResult = await shelby.retrieve({
          cid: params.cid as string,
        });
        sendResponse(successResponse(id, retrieveResult));
        break;
      }

      case "shelby.unpin": {
        if (!params || typeof params.cid !== "string") {
          sendResponse(
            errorResponse(id, ERR_INTERNAL, "Missing cid parameter")
          );
          break;
        }
        const unpinResult = await shelby.unpin({ cid: params.cid as string });
        sendResponse(successResponse(id, unpinResult));
        break;
      }

      case "shelby.list": {
        const listResult = await shelby.list();
        sendResponse(successResponse(id, listResult));
        break;
      }

      default:
        sendResponse(
          errorResponse(id, ERR_METHOD_NOT_FOUND, `Unknown method: ${method}`)
        );
    }
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    sendResponse(errorResponse(id, ERR_INTERNAL, message));
  }
}

// ---------------------------------------------------------------------------
// Stdin line reader
// ---------------------------------------------------------------------------

async function main(): Promise<void> {
  await shelby.initialize(config);

  // Log to stderr (stdout is reserved for JSON-RPC)
  console.error("[sidecar] ShelDrive sidecar started");
  console.error("[sidecar] Network:", config.network);
  console.error("[sidecar] RPC URL:", config.rpcBaseUrl ?? "(default)");

  const rl = readline.createInterface({
    input: process.stdin,
    terminal: false,
  });

  rl.on("line", (line: string) => {
    if (!line.trim()) return;

    let req: JsonRpcRequest;
    try {
      req = JSON.parse(line);
    } catch {
      sendResponse(errorResponse(0, ERR_PARSE, "Invalid JSON"));
      return;
    }

    if (req.jsonrpc !== "2.0" || typeof req.id !== "number" || !req.method) {
      sendResponse(
        errorResponse(req.id ?? 0, ERR_PARSE, "Invalid JSON-RPC request")
      );
      return;
    }

    handleRequest(req).catch((err) => {
      console.error("[sidecar] Unhandled error:", err);
      sendResponse(
        errorResponse(req.id, ERR_INTERNAL, "Unhandled internal error")
      );
    });
  });

  rl.on("close", () => {
    console.error("[sidecar] stdin closed — exiting");
    process.exit(0);
  });

  process.on("SIGTERM", () => {
    console.error("[sidecar] SIGTERM received — exiting");
    process.exit(0);
  });

  process.on("SIGINT", () => {
    console.error("[sidecar] SIGINT received — exiting");
    process.exit(0);
  });
}

main().catch((err) => {
  console.error("[sidecar] Fatal error:", err);
  process.exit(1);
});
