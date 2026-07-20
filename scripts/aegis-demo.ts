// Live aegis demo on devnet: stand up an issuer and attest the user's wallet.
// Run: RPC_URL="<helius>" tsx scripts/aegis-demo.ts
import { readFileSync } from "node:fs";
import { homedir } from "node:os";
import * as anchor from "@coral-xyz/anchor";
import { AnchorProvider, Program, Wallet } from "@coral-xyz/anchor";
import BN from "bn.js";
import { Connection, Keypair, PublicKey } from "@solana/web3.js";

const RPC = process.env.RPC_URL ?? "https://api.devnet.solana.com";
const SUBJECT = new PublicKey("ETasZGB4CX2Nbv3H2L3AKuMqWHdQrq8vhWkzJ4T6kLhN"); // user's Phantom wallet
const REGION_SCHEMA = 1; // aegis::constants::schema::REGION
const EU_BIT = 0b0010;

const idl = JSON.parse(readFileSync("idl/aegis.json", "utf8"));
const secret = JSON.parse(
  readFileSync(`${homedir()}/.config/solana/id.json`, "utf8"),
);
const payer = Keypair.fromSecretKey(Uint8Array.from(secret));
const connection = new Connection(RPC, "confirmed");
const provider = new AnchorProvider(connection, new Wallet(payer), {
  commitment: "confirmed",
});
anchor.setProvider(provider);
const program = new Program(idl, provider);
const pid = program.programId;

const link = (sig: string) => `https://explorer.solana.com/tx/${sig}?cluster=devnet`;
const acct = (a: PublicKey) =>
  `https://explorer.solana.com/address/${a.toBase58()}?cluster=devnet`;

const [issuer] = PublicKey.findProgramAddressSync(
  [Buffer.from("issuer"), payer.publicKey.toBuffer()],
  pid,
);
const [attestation] = PublicKey.findProgramAddressSync(
  [Buffer.from("attestation"), issuer.toBuffer(), SUBJECT.toBuffer()],
  pid,
);

async function main() {
  console.log("aegis program :", pid.toBase58());
  console.log("issuer PDA    :", issuer.toBase58());
  console.log("subject wallet:", SUBJECT.toBase58());

  // 1) Issuer (idempotent).
  const existing = await connection.getAccountInfo(issuer);
  if (!existing) {
    const sig = await program.methods
      .initIssuer("VESTA Geo Oracle")
      .accounts({ authority: payer.publicKey, issuer })
      .rpc();
    console.log("\ninit_issuer   :", link(sig));
  } else {
    console.log("\ninit_issuer   : (already exists, skipping)");
  }

  // 2) Attestation for the user's wallet: region = EU, never expires.
  const data = {
    schema: REGION_SCHEMA,
    value: new BN(EU_BIT),
    validFrom: new BN(0),
    expiresAt: new BN(0),
  };
  const already = await connection.getAccountInfo(attestation);
  if (already) {
    const sig = await program.methods
      .updateAttestation(data)
      .accounts({ signer: payer.publicKey, issuer, attestation })
      .rpc();
    console.log("update_attest :", link(sig));
  } else {
    const sig = await program.methods
      .issueAttestation(SUBJECT, data)
      .accounts({ signer: payer.publicKey, issuer, attestation })
      .rpc();
    console.log("issue_attest  :", link(sig));
  }

  console.log("\nAttestation account:", acct(attestation));
  console.log("Region bitmask     :", EU_BIT, "(EU)");
}

main().then(
  () => process.exit(0),
  (e) => {
    console.error(e);
    process.exit(1);
  },
);
