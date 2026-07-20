/**
 * Issue points from one seeded merchant to a wallet (merchant-signed / gasless
 * for the recipient). Defaults to Palestra Fitness (merchant id 3) → your
 * Phantom wallet. Run:  RPC_URL="…" npx tsx scripts/earn-to-me.ts
 */
import { createHash } from 'node:crypto'
import { readFileSync } from 'node:fs'
import { homedir } from 'node:os'
import { join } from 'node:path'
import {
  ASSOCIATED_TOKEN_PROGRAM_ID as ATOKEN,
  getAssociatedTokenAddressSync,
  TOKEN_2022_PROGRAM_ID as T22,
} from '@solana/spl-token'
import {
  ComputeBudgetProgram,
  Connection,
  Keypair,
  PublicKey,
  sendAndConfirmTransaction,
  SystemProgram,
  Transaction,
  TransactionInstruction,
} from '@solana/web3.js'

const RPC = process.env.RPC_URL ?? 'https://api.devnet.solana.com'
const VESTA_CORE = new PublicKey('gaMq6BpH1aqC8ZCYtAxwZBjTa9AnfdWvYwURG6L4LDz')
const CUSTOMER = new PublicKey(process.env.CUSTOMER ?? 'ETasZGB4CX2Nbv3H2L3AKuMqWHdQrq8vhWkzJ4T6kLhN')
const MERCHANT_ID = BigInt(process.env.MERCHANT_ID ?? '3') // Palestra Fitness
const AMOUNT = BigInt(process.env.AMOUNT ?? '6000') // base spend units

const enc = new TextEncoder()
const disc = (n: string) => createHash('sha256').update(`global:${n}`).digest().subarray(0, 8)
const u64 = (n: bigint) => {
  const b = Buffer.alloc(8)
  b.writeBigUInt64LE(n)
  return b
}
const u32 = (n: number) => {
  const b = Buffer.alloc(4)
  b.writeUInt32LE(n)
  return b
}
const pda = (seeds: (Buffer | Uint8Array)[]) => PublicKey.findProgramAddressSync(seeds, VESTA_CORE)[0]
const meta = (pubkey: PublicKey, s: boolean, w: boolean) => ({ pubkey, isSigner: s, isWritable: w })

async function main() {
  const connection = new Connection(RPC, 'confirmed')
  const dev = Keypair.fromSecretKey(
    Uint8Array.from(JSON.parse(readFileSync(join(homedir(), '.config/solana/id.json'), 'utf8'))),
  )
  const merchant = pda([enc.encode('merchant'), dev.publicKey.toBuffer(), u64(MERCHANT_ID)])
  const mint = pda([enc.encode('mint'), merchant.toBuffer()])
  const profile = pda([enc.encode('customer'), merchant.toBuffer(), CUSTOMER.toBuffer()])
  const ata = getAssociatedTokenAddressSync(mint, CUSTOMER, true, T22)
  const config = pda([enc.encode('config')])
  const visitDay = Math.floor(Date.now() / 1000 / 86_400)

  const ix = new TransactionInstruction({
    programId: VESTA_CORE,
    keys: [
      meta(dev.publicKey, true, true),
      meta(merchant, false, true),
      meta(CUSTOMER, false, false),
      meta(profile, false, true),
      meta(mint, false, true),
      meta(ata, false, true),
      meta(config, false, false),
      meta(T22, false, false),
      meta(ATOKEN, false, false),
      meta(SystemProgram.programId, false, false),
    ],
    data: Buffer.concat([disc('earn_points'), u64(AMOUNT), u32(visitDay)]),
  })

  const sig = await sendAndConfirmTransaction(
    connection,
    new Transaction().add(ComputeBudgetProgram.setComputeUnitLimit({ units: 400_000 }), ix),
    [dev],
    { commitment: 'confirmed' },
  )
  console.log(`merchant id ${MERCHANT_ID}  mint ${mint.toBase58()}`)
  console.log(`issued ${AMOUNT} base units to ${CUSTOMER.toBase58()}`)
  console.log(`tx  https://explorer.solana.com/tx/${sig}?cluster=devnet`)
  console.log(`ata https://explorer.solana.com/address/${ata.toBase58()}?cluster=devnet`)
}

main().then(
  () => process.exit(0),
  (e) => {
    console.error(e)
    process.exit(1)
  },
)
