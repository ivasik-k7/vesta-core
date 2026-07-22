// Live devnet visibility: register a demo merchant (dev-key authority, so it's
// reusable after the argus/vesta_core upgrades) and mint points into the user's
// Phantom wallet. Minting does not trigger the transfer hook, so this works on
// the currently deployed vesta_core without the guard.
// Run: RPC_URL="<helius>" tsx scripts/live-demo.ts
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import {
  ComputeBudgetProgram,
  Connection,
  Keypair,
  PublicKey,
  SystemProgram,
  Transaction,
  TransactionInstruction,
  sendAndConfirmTransaction,
} from "@solana/web3.js";
import {
  ASSOCIATED_TOKEN_PROGRAM_ID,
  TOKEN_2022_PROGRAM_ID as T22,
  getAssociatedTokenAddressSync,
} from "@solana/spl-token";

const RPC = process.env.RPC_URL ?? "https://api.devnet.solana.com";
const VESTA_CORE = new PublicKey("gaMq6BpH1aqC8ZCYtAxwZBjTa9AnfdWvYwURG6L4LDz");
const CUSTOMER = new PublicKey("ETasZGB4CX2Nbv3H2L3AKuMqWHdQrq8vhWkzJ4T6kLhN");
const enc = new TextEncoder();

const disc = (n: string) => createHash("sha256").update(`global:${n}`).digest().subarray(0, 8);
const u64 = (n: bigint) => { const b = Buffer.alloc(8); b.writeBigUInt64LE(n); return b; };
const u32 = (n: number) => { const b = Buffer.alloc(4); b.writeUInt32LE(n); return b; };
const i16 = (n: number) => { const b = Buffer.alloc(2); b.writeInt16LE(n); return b; };
const bstr = (s: string) => { const body = enc.encode(s); return Buffer.concat([u32(body.length), Buffer.from(body)]); };
const pda = (seeds: (Buffer | Uint8Array)[], p = VESTA_CORE) => PublicKey.findProgramAddressSync(seeds, p)[0];
const meta = (pubkey: PublicKey, s: boolean, w: boolean) => ({ pubkey, isSigner: s, isWritable: w });
const link = (sig: string) => `https://explorer.solana.com/tx/${sig}?cluster=devnet`;
const addr = (a: PublicKey) => `https://explorer.solana.com/address/${a.toBase58()}?cluster=devnet`;

async function main() {
  const connection = new Connection(RPC, "confirmed");
  const dev = Keypair.fromSecretKey(
    Uint8Array.from(JSON.parse(readFileSync(join(homedir(), ".config/solana/id.json"), "utf8"))),
  );
  const config = pda([enc.encode("config")]);
  const merchant = pda([enc.encode("merchant"), dev.publicKey.toBuffer(), u64(0n)]);
  const mint = pda([enc.encode("mint"), merchant.toBuffer()]);
  const treasury = getAssociatedTokenAddressSync(mint, dev.publicKey, false, T22);
  const budget = ComputeBudgetProgram.setComputeUnitLimit({ units: 400_000 });

  const send = async (label: string, ix: TransactionInstruction) => {
    const sig = await sendAndConfirmTransaction(
      connection,
      new Transaction().add(budget, ix),
      [dev],
      { commitment: "confirmed" },
    );
    console.log(`  ✓ ${label}\n    ${link(sig)}`);
  };

  console.log("merchant :", merchant.toBase58());
  console.log("mint     :", mint.toBase58());
  console.log("customer :", CUSTOMER.toBase58());

  if (!(await connection.getAccountInfo(merchant))) {
    await send(
      "register_merchant (VESTA Demo)",
      new TransactionInstruction({
        programId: VESTA_CORE,
        keys: [
          meta(dev.publicKey, true, true),
          meta(merchant, false, true),
          meta(mint, false, true),
          meta(treasury, false, true),
          meta(config, false, false),
          meta(T22, false, false),
          meta(ASSOCIATED_TOKEN_PROGRAM_ID, false, false),
          meta(SystemProgram.programId, false, false),
        ],
        data: Buffer.concat([
          disc("register_merchant"),
          u64(0n), // merchant id (multi-record)
          bstr("VESTA Demo"),
          bstr("VESTA"),
          bstr("https://dev-vesta.netlify.app/points.json"),
          i16(-2000),
          u64(100n),
          Buffer.from([2]),
        ]),
      }),
    );
  } else {
    console.log("  · merchant already registered, skipping");
  }

  // Mint points to the user's wallet (merchant-signed, customer is gasless).
  const today = Math.floor(Date.now() / 1000 / 86_400);
  const profile = pda([enc.encode("customer"), merchant.toBuffer(), CUSTOMER.toBuffer()]);
  const ata = getAssociatedTokenAddressSync(mint, CUSTOMER, false, T22);
  await send(
    "earn_points (5,000.00 → your wallet)",
    new TransactionInstruction({
      programId: VESTA_CORE,
      keys: [
        meta(dev.publicKey, true, true),
        meta(merchant, false, true),
        meta(CUSTOMER, false, false),
        meta(profile, false, true),
        meta(mint, false, true),
        meta(ata, false, true),
        meta(config, false, false),
        meta(VESTA_CORE, false, false), // merchant_segments: None
        meta(VESTA_CORE, false, false), // customer_eligibility: None
        meta(T22, false, false),
        meta(ASSOCIATED_TOKEN_PROGRAM_ID, false, false),
        meta(SystemProgram.programId, false, false),
      ],
      data: Buffer.concat([disc("earn_points"), u64(5_000n), u32(today)]),
    }),
  );

  console.log("\nPoint token :", addr(mint));
  console.log("Your ATA    :", addr(ata));
}

main().then(() => process.exit(0), (e) => { console.error(e); process.exit(1); });
