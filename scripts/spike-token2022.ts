// Phase 0 spike: prove the full VESTA extension stack composes on one
// Token-2022 mint (metadata + interest-bearing decay + transfer hook +
// permanent delegate), plus a soulbound badge mint. Run against devnet:
//   npx tsx scripts/spike-token2022.ts

import { readFileSync } from 'node:fs'
import { homedir } from 'node:os'
import { join } from 'node:path'
import {
  Connection,
  Keypair,
  PublicKey,
  SystemProgram,
  Transaction,
  sendAndConfirmTransaction,
} from '@solana/web3.js'
import {
  AuthorityType,
  ExtensionType,
  LENGTH_SIZE,
  TOKEN_2022_PROGRAM_ID,
  TYPE_SIZE,
  createAssociatedTokenAccountIdempotentInstruction,
  createInitializeInterestBearingMintInstruction,
  createInitializeMetadataPointerInstruction,
  createInitializeMintInstruction,
  createInitializeNonTransferableMintInstruction,
  createInitializePermanentDelegateInstruction,
  createInitializeTransferHookInstruction,
  createMintToInstruction,
  createSetAuthorityInstruction,
  getAssociatedTokenAddressSync,
  getMintLen,
} from '@solana/spl-token'
import { createInitializeInstruction, pack, type TokenMetadata } from '@solana/spl-token-metadata'

const RPC_URL = process.env.RPC_URL ?? 'https://api.devnet.solana.com'
const DECIMALS = 2
// -20% APR: points cool down unless the customer keeps the flame alive
const DECAY_RATE_BPS = -2000
// Placeholder until argus is deployed; hook mints are created in phase 2
const ARGUS_PLACEHOLDER = new PublicKey('Fg6PaFpoGXkYsidMpWTK6W2BeZ7FEfcYkg476zPFsLnS')

function loadPayer(): Keypair {
  const raw = readFileSync(join(homedir(), '.config/solana/id.json'), 'utf8')
  return Keypair.fromSecretKey(Uint8Array.from(JSON.parse(raw)))
}

function explorer(sig: string): string {
  return `https://explorer.solana.com/tx/${sig}?cluster=devnet`
}

async function createPointsMint(connection: Connection, payer: Keypair): Promise<void> {
  const mint = Keypair.generate()
  const metadata: TokenMetadata = {
    mint: mint.publicKey,
    name: 'Vesta Demo Points',
    symbol: 'FLAME',
    uri: 'https://vesta.example/points.json',
    additionalMetadata: [['merchant', 'spike-demo']],
  }

  const extensions = [
    ExtensionType.MetadataPointer,
    ExtensionType.InterestBearingConfig,
    ExtensionType.TransferHook,
    ExtensionType.PermanentDelegate,
  ]
  const mintLen = getMintLen(extensions)
  const metadataLen = TYPE_SIZE + LENGTH_SIZE + pack(metadata).length
  const lamports = await connection.getMinimumBalanceForRentExemption(mintLen + metadataLen)

  const tx = new Transaction().add(
    SystemProgram.createAccount({
      fromPubkey: payer.publicKey,
      newAccountPubkey: mint.publicKey,
      space: mintLen,
      lamports,
      programId: TOKEN_2022_PROGRAM_ID,
    }),
    // Extensions must be initialized before the mint itself
    createInitializeMetadataPointerInstruction(
      mint.publicKey, payer.publicKey, mint.publicKey, TOKEN_2022_PROGRAM_ID,
    ),
    createInitializeInterestBearingMintInstruction(
      mint.publicKey, payer.publicKey, DECAY_RATE_BPS, TOKEN_2022_PROGRAM_ID,
    ),
    createInitializeTransferHookInstruction(
      mint.publicKey, payer.publicKey, ARGUS_PLACEHOLDER, TOKEN_2022_PROGRAM_ID,
    ),
    createInitializePermanentDelegateInstruction(
      mint.publicKey, payer.publicKey, TOKEN_2022_PROGRAM_ID,
    ),
    createInitializeMintInstruction(
      mint.publicKey, DECIMALS, payer.publicKey, null, TOKEN_2022_PROGRAM_ID,
    ),
    createInitializeInstruction({
      programId: TOKEN_2022_PROGRAM_ID,
      mint: mint.publicKey,
      metadata: mint.publicKey,
      name: metadata.name,
      symbol: metadata.symbol,
      uri: metadata.uri,
      mintAuthority: payer.publicKey,
      updateAuthority: payer.publicKey,
    }),
  )
  const sig = await sendAndConfirmTransaction(connection, tx, [payer, mint])
  console.log(`points mint  ${mint.publicKey.toBase58()}`)
  console.log(`  create     ${explorer(sig)}`)

  const ata = getAssociatedTokenAddressSync(
    mint.publicKey, payer.publicKey, false, TOKEN_2022_PROGRAM_ID,
  )
  const mintTx = new Transaction().add(
    createAssociatedTokenAccountIdempotentInstruction(
      payer.publicKey, ata, payer.publicKey, mint.publicKey, TOKEN_2022_PROGRAM_ID,
    ),
    createMintToInstruction(
      mint.publicKey, ata, payer.publicKey, 100_00n, [], TOKEN_2022_PROGRAM_ID,
    ),
  )
  const mintSig = await sendAndConfirmTransaction(connection, mintTx, [payer])
  console.log(`  mint 100   ${explorer(mintSig)}`)
}

async function createBadgeMint(connection: Connection, payer: Keypair): Promise<void> {
  const mint = Keypair.generate()
  const metadata: TokenMetadata = {
    mint: mint.publicKey,
    name: 'Kleos Badge: First Flame',
    symbol: 'KLEOS',
    uri: 'https://vesta.example/badges/first-flame.json',
    additionalMetadata: [['tier', 'bronze']],
  }

  const extensions = [ExtensionType.NonTransferable, ExtensionType.MetadataPointer]
  const mintLen = getMintLen(extensions)
  const metadataLen = TYPE_SIZE + LENGTH_SIZE + pack(metadata).length
  const lamports = await connection.getMinimumBalanceForRentExemption(mintLen + metadataLen)

  const ata = getAssociatedTokenAddressSync(
    mint.publicKey, payer.publicKey, false, TOKEN_2022_PROGRAM_ID,
  )
  const tx = new Transaction().add(
    SystemProgram.createAccount({
      fromPubkey: payer.publicKey,
      newAccountPubkey: mint.publicKey,
      space: mintLen,
      lamports,
      programId: TOKEN_2022_PROGRAM_ID,
    }),
    createInitializeNonTransferableMintInstruction(mint.publicKey, TOKEN_2022_PROGRAM_ID),
    createInitializeMetadataPointerInstruction(
      mint.publicKey, payer.publicKey, mint.publicKey, TOKEN_2022_PROGRAM_ID,
    ),
    createInitializeMintInstruction(
      mint.publicKey, 0, payer.publicKey, null, TOKEN_2022_PROGRAM_ID,
    ),
    createInitializeInstruction({
      programId: TOKEN_2022_PROGRAM_ID,
      mint: mint.publicKey,
      metadata: mint.publicKey,
      name: metadata.name,
      symbol: metadata.symbol,
      uri: metadata.uri,
      mintAuthority: payer.publicKey,
      updateAuthority: payer.publicKey,
    }),
    createAssociatedTokenAccountIdempotentInstruction(
      payer.publicKey, ata, payer.publicKey, mint.publicKey, TOKEN_2022_PROGRAM_ID,
    ),
    createMintToInstruction(mint.publicKey, ata, payer.publicKey, 1n, [], TOKEN_2022_PROGRAM_ID),
    // Badge is 1-of-1 and soulbound: freeze supply forever
    createSetAuthorityInstruction(
      mint.publicKey, payer.publicKey, AuthorityType.MintTokens, null, [], TOKEN_2022_PROGRAM_ID,
    ),
  )
  const sig = await sendAndConfirmTransaction(connection, tx, [payer, mint])
  console.log(`badge mint   ${mint.publicKey.toBase58()}`)
  console.log(`  create     ${explorer(sig)}`)
}

async function main(): Promise<void> {
  const connection = new Connection(RPC_URL, 'confirmed')
  const payer = loadPayer()
  const balance = await connection.getBalance(payer.publicKey)
  console.log(`payer ${payer.publicKey.toBase58()} balance ${balance / 1e9} SOL`)
  if (balance < 0.1 * 1e9) {
    throw new Error('Payer balance too low — fund via https://faucet.solana.com')
  }
  await createPointsMint(connection, payer)
  await createBadgeMint(connection, payer)
  console.log('spike OK: all extensions composed')
}

main().catch((err) => {
  console.error(err)
  process.exit(1)
})
