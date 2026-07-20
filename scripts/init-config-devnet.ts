// One-shot: initialize the VESTA Config PDA on devnet.
// Builds the instruction manually (anchor discriminator + accounts),
// so it needs no anchor TS client. Run: npx tsx scripts/init-config-devnet.ts

import { createHash } from 'node:crypto'
import { readFileSync } from 'node:fs'
import { homedir } from 'node:os'
import { join } from 'node:path'
import {
  Connection,
  Keypair,
  PublicKey,
  SystemProgram,
  Transaction,
  TransactionInstruction,
  sendAndConfirmTransaction,
} from '@solana/web3.js'

const RPC_URL = process.env.RPC_URL ?? 'https://api.devnet.solana.com'
const VESTA_CORE = new PublicKey('gaMq6BpH1aqC8ZCYtAxwZBjTa9AnfdWvYwURG6L4LDz')

function discriminator(ixName: string): Buffer {
  return createHash('sha256').update(`global:${ixName}`).digest().subarray(0, 8)
}

async function main(): Promise<void> {
  const connection = new Connection(RPC_URL, 'confirmed')
  const raw = readFileSync(join(homedir(), '.config/solana/id.json'), 'utf8')
  const admin = Keypair.fromSecretKey(Uint8Array.from(JSON.parse(raw)))
  const [config] = PublicKey.findProgramAddressSync([Buffer.from('config')], VESTA_CORE)

  const existing = await connection.getAccountInfo(config)
  if (existing) {
    console.log(`config already initialized: ${config.toBase58()}`)
    return
  }

  const ix = new TransactionInstruction({
    programId: VESTA_CORE,
    keys: [
      { pubkey: admin.publicKey, isSigner: true, isWritable: true },
      { pubkey: config, isSigner: false, isWritable: true },
      { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
    ],
    data: discriminator('init_config'),
  })

  const sig = await sendAndConfirmTransaction(connection, new Transaction().add(ix), [admin])
  console.log(`config PDA   ${config.toBase58()}`)
  console.log(`init_config  https://explorer.solana.com/tx/${sig}?cluster=devnet`)
}

main().catch((err) => {
  console.error(err)
  process.exit(1)
})
