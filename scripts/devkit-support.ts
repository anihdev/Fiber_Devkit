// Shared helpers for Fiber DevKit testnet funding support scripts.
// These scripts are not part of the FNN runtime and are not included in the published npm package.
// They are only used by the `pnpm fund:nodes` command to fund generated DevKit node keys with testnet CKB.
// They are not intended for production use and should not be used in any real-world application.

import { ccc } from "@ckb-ccc/core";
import { readdir, readFile, stat } from "node:fs/promises";
import path from "node:path";

export const DEFAULT_CKB_RPC_URL = "https://testnet.ckb.dev/rpc";
export const DEFAULT_NODE_FUND_CKB = "500";

export type NodeKey = {
  name: string;
  privateKey: string;
  keyPath: string;
};

export type NodeBalance = {
  node: NodeKey;
  address: string;
  lock: ccc.Script;
  balance: bigint;
};

export type RetryOptions = {
  attempts?: number;
  baseDelayMs?: number;
};

export function repoPath(...parts: string[]): string {
  return path.resolve(process.cwd(), ...parts);
}

export async function readGeneratedNodeKeys(): Promise<NodeKey[]> {
  const nodesDir = repoPath(".fiber", "nodes");

  try {
    const info = await stat(nodesDir);
    if (!info.isDirectory()) {
      throw new Error(`${nodesDir} is not a directory`);
    }
  } catch (error) {
    throw new Error(
      "No generated FNN node keys found. Run `fiber init --nodes 3 --template hub-spoke` first.",
      { cause: error },
    );
  }

  const names = (await readdir(nodesDir, { withFileTypes: true }))
    .filter((entry) => entry.isDirectory())
    .map((entry) => entry.name)
    .sort((left, right) => left.localeCompare(right));

  const nodes: NodeKey[] = [];
  for (const name of names) {
    const keyPath = path.join(nodesDir, name, "ckb", "key");
    try {
      const privateKey = normalizePrivateKey(await readFile(keyPath, "utf8"));
      nodes.push({ name, privateKey, keyPath });
    } catch (error) {
      throw new Error(`Failed to read generated CKB key for ${name} at ${keyPath}`, {
        cause: error,
      });
    }
  }

  if (nodes.length === 0) {
    throw new Error(
      "No generated FNN node keys found. Run `fiber init --nodes 3 --template hub-spoke` first.",
    );
  }

  return nodes;
}

export function createTestnetClient(rpcUrl = DEFAULT_CKB_RPC_URL): ccc.ClientPublicTestnet {
  return new ccc.ClientPublicTestnet({ url: rpcUrl });
}

export function signerForNode(
  client: ccc.Client,
  node: NodeKey,
): ccc.SignerCkbPrivateKey {
  return new ccc.SignerCkbPrivateKey(client, node.privateKey);
}

export async function nodeBalance(
  client: ccc.Client,
  node: NodeKey,
): Promise<NodeBalance> {
  const signer = signerForNode(client, node);
  const address = await withRetry(() => signer.getRecommendedAddress());
  const { script } = await withRetry(() => signer.getRecommendedAddressObj());
  const balance = await withRetry(() => signer.getBalance());

  return { node, address, lock: script, balance };
}

export async function nodeBalances(
  client: ccc.Client,
  nodes: NodeKey[],
): Promise<NodeBalance[]> {
  const balances: NodeBalance[] = [];
  for (const node of nodes) {
    balances.push(await nodeBalance(client, node));
  }
  return balances;
}

export function ckbToShannons(value: string): bigint {
  return ccc.fixedPointFrom(value.trim(), 8);
}

export function shannonsToCkb(value: bigint): string {
  return ccc.fixedPointToString(value, 8);
}

export function formatCkb(value: bigint): string {
  return `${shannonsToCkb(value)} CKB`;
}

export async function withRetry<T>(
  operation: () => Promise<T>,
  options: RetryOptions = {},
): Promise<T> {
  const attempts = options.attempts ?? 5;
  const baseDelayMs = options.baseDelayMs ?? 500;

  let lastError: unknown;
  for (let attempt = 1; attempt <= attempts; attempt += 1) {
    try {
      return await operation();
    } catch (error) {
      lastError = error;
      if (attempt === attempts || !isTransientRpcError(error)) {
        throw error;
      }

      await delay(baseDelayMs * attempt);
    }
  }

  throw lastError instanceof Error ? lastError : new Error(String(lastError));
}

export async function waitForTransaction(
  client: ccc.Client,
  txHash: string,
  timeoutMs = 600_000,
): Promise<ccc.ClientTransactionResponse> {
  const startedAt = Date.now();

  while (Date.now() - startedAt < timeoutMs) {
    const response = await withRetry(() => client.getTransactionNoCache(txHash));
    if (response?.status === "committed") {
      return response;
    }

    if (response?.status === "rejected") {
      throw new Error(`Funding transaction ${txHash} was rejected: ${response.reason}`);
    }

    await delay(3_000);
  }

  throw new Error(`Timed out waiting for funding transaction ${txHash} to commit`);
}

export function errorMessage(error: unknown): string {
  if (error instanceof Error) {
    const cause = error.cause ? errorMessage(error.cause) : "";
    if (cause && cause !== error.message) {
      return `${error.message}: ${cause}`;
    }
    return error.message;
  }
  return String(error);
}

function normalizePrivateKey(contents: string): string {
  const trimmed = contents.trim();
  if (/^0x[0-9a-fA-F]{64}$/.test(trimmed)) {
    return trimmed.toLowerCase();
  }

  if (/^[0-9a-fA-F]{64}$/.test(trimmed)) {
    return `0x${trimmed.toLowerCase()}`;
  }

  throw new Error(
    "CKB key file is not a generated 32-byte hex private key. Run funding scripts after `fiber init` or `fiber reset`, before FNN rewrites its key storage.",
  );
}

function isTransientRpcError(error: unknown): boolean {
  const message = errorMessage(error).toLowerCase();
  return [
    "fetch failed",
    "timeout",
    "timed out",
    "eai_again",
    "enotfound",
    "econnreset",
    "econnrefused",
    "socket hang up",
    "temporarily unavailable",
  ].some((needle) => message.includes(needle));
}

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}
