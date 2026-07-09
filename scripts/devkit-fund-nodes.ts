// Tops generated Fiber DevKit node keys up with testnet CKB from a treasury key.

import { ccc } from "@ckb-ccc/core";
import "dotenv/config";
import {
  DEFAULT_CKB_RPC_URL,
  DEFAULT_NODE_FUND_CKB,
  ckbToShannons,
  createTestnetClient,
  errorMessage,
  formatCkb,
  nodeBalances,
  readGeneratedNodeKeys,
  shannonsToCkb,
  waitForTransaction,
  withRetry,
} from "./devkit-support.js";

const FEE_BUFFER = 1_00000000n;
const MIN_SECP256K1_CELL_CAPACITY = 61_00000000n;

type TopUp = {
  name: string;
  address: string;
  lock: ccc.Script;
  current: bigint;
  amount: bigint;
};

async function main(): Promise<void> {
  const rpcUrl = process.env.CKB_RPC_URL ?? DEFAULT_CKB_RPC_URL;
  const targetBalance = readTargetBalance();
  const client = createTestnetClient(rpcUrl);
  const nodes = await readGeneratedNodeKeys();
  const treasuryPrivateKey = normalizeTreasuryPrivateKey(process.env.CKB_PRIVATE_KEY);
  const balances = await nodeBalances(client, nodes);

  const topUps = balances
    .filter((balance) => balance.balance < targetBalance)
    .map<TopUp>((balance) => ({
      name: balance.node.name,
      address: balance.address,
      lock: balance.lock,
      current: balance.balance,
      amount: maxBigInt(targetBalance - balance.balance, MIN_SECP256K1_CELL_CAPACITY),
    }));

  console.log(`CKB RPC: ${rpcUrl}`);
  console.log(`Target node balance: ${formatCkb(targetBalance)}`);

  if (topUps.length === 0) {
    console.log("All generated nodes already meet or exceed the target balance.");
    return;
  }

  const treasury = new ccc.SignerCkbPrivateKey(client, treasuryPrivateKey);
  const treasuryAddress = await withRetry(() => treasury.getRecommendedAddress());
  const treasuryBalance = await withRetry(() => treasury.getBalance());
  const requested = topUps.reduce((sum, topUp) => sum + topUp.amount, 0n);

  console.log(`Treasury address: ${treasuryAddress}`);
  console.log(`Treasury balance: ${formatCkb(treasuryBalance)}`);
  console.log("Planned top-ups:");
  for (const topUp of topUps) {
    console.log(
      `${topUp.name}\t${topUp.address}\tcurrent=${formatCkb(topUp.current)}\ttop_up=${formatCkb(topUp.amount)}`,
    );
  }

  if (treasuryBalance < requested + FEE_BUFFER) {
    throw new Error(
      `Treasury balance is too low. Need at least ${formatCkb(
        requested + FEE_BUFFER,
      )}; available ${formatCkb(treasuryBalance)}.`,
    );
  }

  const tx = await withRetry(() => buildFundingTransaction(treasury, topUps));
  const signedTx = await treasury.signTransaction(tx);
  const txHash = signedTx.hash();

  let appeared: ccc.ClientTransactionResponse | undefined;
  try {
    const sentHash = await withRetry(() => client.sendTransaction(signedTx));
    if (sentHash !== txHash) {
      throw new Error(`CKB RPC returned unexpected tx hash ${sentHash}`);
    }
  } catch (error) {
    // If the RPC accepted the transaction but the response was lost, the expected hash
    // should still become visible through direct transaction polling.
    appeared = await waitForTransaction(client, txHash, 30_000).catch(() => undefined);
    if (!appeared) {
      throw error;
    }
  }

  console.log(`Funding transaction submitted: ${txHash}`);

  appeared ??= await waitForTransaction(client, txHash);
  console.log(`Funding transaction status: ${appeared.status}`);

  const updated = await nodeBalances(client, nodes);
  console.log("Updated node balances:");
  for (const balance of updated) {
    console.log(
      `${balance.node.name}\t${balance.address}\t${formatCkb(balance.balance)}`,
    );
  }
}

async function buildFundingTransaction(
  treasury: ccc.SignerCkbPrivateKey,
  topUps: TopUp[],
): Promise<ccc.Transaction> {
  const tx = ccc.Transaction.from({
    outputs: topUps.map((topUp) => ({
      capacity: topUp.amount,
      lock: topUp.lock,
    })),
    outputsData: topUps.map(() => "0x"),
  });

  // Let CCC collect live treasury cells and create change back to the treasury key.
  await tx.completeInputsByCapacity(treasury);
  await tx.completeFeeBy(treasury);
  return tx;
}

function maxBigInt(left: bigint, right: bigint): bigint {
  return left > right ? left : right;
}

function readTargetBalance(): bigint {
  const rawValue = process.env.DEVKIT_NODE_FUND_CKB ?? DEFAULT_NODE_FUND_CKB;
  try {
    const value = ckbToShannons(rawValue);
    if (value <= 0n) {
      throw new Error("must be positive");
    }
    return value;
  } catch (error) {
    throw new Error(
      `DEVKIT_NODE_FUND_CKB must be a positive CKB amount, got ${JSON.stringify(rawValue)} (${errorMessage(error)})`,
    );
  }
}

function normalizeTreasuryPrivateKey(value: string | undefined): string {
  if (!value?.trim()) {
    throw new Error(
      "CKB_PRIVATE_KEY is required in repo-root .env for `pnpm fund:nodes`.",
    );
  }

  const trimmed = value.trim();
  if (!/^0x[0-9a-fA-F]{64}$/.test(trimmed)) {
    throw new Error("CKB_PRIVATE_KEY must be a 32-byte 0x-prefixed hex string.");
  }

  return trimmed.toLowerCase();
}

main().catch((error: unknown) => {
  console.error(`fund:nodes failed: ${errorMessage(error)}`);
  process.exitCode = 1;
});
