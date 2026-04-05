/**
 * Wrapper around the Shelby Protocol SDK.
 * Provides upload/download operations mapped to filesystem semantics.
 *
 * Falls back to a mock implementation if the SDK is not available or
 * credentials are not configured, allowing development without a live node.
 */

import type {
  PinParams,
  PinResult,
  RetrieveParams,
  RetrieveResult,
  UnpinParams,
  UnpinResult,
  ListResult,
  StatusResult,
} from "./protocol.js";

interface SidecarConfig {
  network: string;
  rpcBaseUrl: string | null;
  apiKey: string | null;
  privateKey: string | null;
}

// In-memory store for mock mode
const mockStore = new Map<string, Buffer>();
let mockCidCounter = 0;

let sdkAvailable = false;
let shelbyClient: any = null;
let signerAccount: any = null;

export async function initialize(config: SidecarConfig): Promise<boolean> {
  try {
    const shelbySDK = await import("@shelby-protocol/sdk/node");
    const aptosSDK = await import("@aptos-labs/ts-sdk");

    if (!config.privateKey) {
      console.error(
        "[shelby-client] No SHELBY_PRIVATE_KEY set — running in mock mode"
      );
      return false;
    }

    // Create signer from private key
    signerAccount = aptosSDK.Account.fromPrivateKey({
      privateKey: new aptosSDK.Ed25519PrivateKey(config.privateKey),
    });

    const clientConfig: any = {
      network: aptosSDK.Network.SHELBYNET,
    };

    if (config.apiKey) {
      clientConfig.apiKey = config.apiKey;
      // Pass API key to all sub-services
      clientConfig.rpc = { ...(clientConfig.rpc ?? {}), apiKey: config.apiKey };
      clientConfig.indexer = { apiKey: config.apiKey };
    }

    if (config.rpcBaseUrl) {
      clientConfig.rpc = { ...(clientConfig.rpc ?? {}), baseUrl: config.rpcBaseUrl };
    }

    shelbyClient = new shelbySDK.ShelbyNodeClient(clientConfig);
    sdkAvailable = true;

    console.error(
      "[shelby-client] Connected to Shelby network as",
      signerAccount.accountAddress.toString()
    );
    return true;
  } catch (err) {
    console.error(
      "[shelby-client] Shelby SDK initialization failed — running in mock mode:",
      (err as Error).message
    );
    sdkAvailable = false;
    return false;
  }
}

export async function pin(params: PinParams): Promise<PinResult> {
  const content = Buffer.from(params.content, "base64");

  if (sdkAvailable && shelbyClient && signerAccount) {
    try {
      const blobName = params.filename ?? `sheldrive/${Date.now()}`;
      // Set expiration to 30 days from now (in microseconds)
      const expirationMicros = (Date.now() + 30 * 24 * 60 * 60 * 1000) * 1000;

      await shelbyClient.upload({
        blobData: new Uint8Array(content),
        signer: signerAccount,
        blobName,
        expirationMicros,
      });

      // Use account address + blob name as CID
      const cid = `shelby:${signerAccount.accountAddress.toString()}/${blobName}`;
      return { cid, size_bytes: content.length };
    } catch (err) {
      throw new Error(`Upload failed: ${(err as Error).message}`);
    }
  }

  // Mock mode
  mockCidCounter++;
  const cid = `mock:bafk${mockCidCounter.toString(16).padStart(12, "0")}`;
  mockStore.set(cid, content);
  return { cid, size_bytes: content.length };
}

export async function retrieve(params: RetrieveParams): Promise<RetrieveResult> {
  if (sdkAvailable && shelbyClient && signerAccount) {
    try {
      // Parse CID format: shelby:<account>/<blobName>
      const match = params.cid.match(/^shelby:(.+?)\/(.+)$/);
      if (!match) {
        throw new Error(`Invalid Shelby CID format: ${params.cid}`);
      }
      const [, account, blobName] = match;

      const blob = await shelbyClient.download({ account, blobName });
      const data = Buffer.from(blob.data);
      return { content: data.toString("base64"), size_bytes: data.length };
    } catch (err) {
      throw new Error(`Download failed: ${(err as Error).message}`);
    }
  }

  // Mock mode
  const data = mockStore.get(params.cid);
  if (!data) {
    throw new Error(`CID not found: ${params.cid}`);
  }
  return { content: data.toString("base64"), size_bytes: data.length };
}

export async function unpin(params: UnpinParams): Promise<UnpinResult> {
  if (sdkAvailable && shelbyClient) {
    // Shelby doesn't have an explicit unpin — blobs expire.
    // For now, we just remove from our local tracking.
    console.error(`[shelby-client] Unpin requested for ${params.cid} — blob will expire naturally`);
    return { success: true };
  }

  // Mock mode
  const existed = mockStore.delete(params.cid);
  return { success: existed };
}

export async function list(): Promise<ListResult> {
  if (sdkAvailable && shelbyClient && signerAccount) {
    try {
      // Use indexer to query blobs for this account
      const indexer = shelbyClient.coordination?.indexer;
      if (indexer) {
        const blobs = await indexer.getBlobs({
          account: signerAccount.accountAddress.toString(),
        });
        const cids = (blobs ?? []).map(
          (b: any) =>
            `shelby:${signerAccount.accountAddress.toString()}/${b.blob_name}`
        );
        return { cids, count: cids.length };
      }
    } catch (err) {
      console.error("[shelby-client] List via indexer failed:", (err as Error).message);
    }
  }

  // Mock mode
  const cids = Array.from(mockStore.keys());
  return { cids, count: cids.length };
}

export function getStatus(config: SidecarConfig): StatusResult {
  return {
    connected: sdkAvailable,
    network: config.network,
    node_url: config.rpcBaseUrl,
  };
}
