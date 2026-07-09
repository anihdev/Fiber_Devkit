// Prints public testnet addresses and balances for generated Fiber DevKit node keys.

import "dotenv/config";
import {
  DEFAULT_CKB_RPC_URL,
  createTestnetClient,
  errorMessage,
  formatCkb,
  nodeBalances,
  readGeneratedNodeKeys,
} from "./devkit-support.js";

async function main(): Promise<void> {
  const rpcUrl = process.env.CKB_RPC_URL ?? DEFAULT_CKB_RPC_URL;
  const client = createTestnetClient(rpcUrl);
  const nodes = await readGeneratedNodeKeys();
  const balances = await nodeBalances(client, nodes);

  console.log(`CKB RPC: ${rpcUrl}`);
  console.log("Node balances:");
  for (const balance of balances) {
    console.log(
      `${balance.node.name}\t${balance.address}\t${formatCkb(balance.balance)}`,
    );
  }
}

main().catch((error: unknown) => {
  console.error(`balances:nodes failed: ${errorMessage(error)}`);
  process.exitCode = 1;
});
