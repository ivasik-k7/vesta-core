/**
 * VESTA master seed — provisions a small, production-shaped demo across all
 * three v2 programs (vesta_core, argus, aegis) on devnet, then runs a
 * verification pass and prints a report.
 *
 * What it creates (idempotent — safe to re-run):
 *   • protocol config (if missing)
 *   • 5 merchants (varied categories / decay), each with:
 *       – an argus transfer guard, 2 offers, 1 campaign, 1 achievement
 *   • 1 koinon alliance with 3 member merchants + swap rates
 *   • 1 aegis issuer + attestations for several wallets
 *   • points issued to ~8 customers (incl. your Phantom wallet), some via campaign
 *   • an offer redemption, a guarded gift, a clawback, a badge grant
 *   • admin verification of 2 merchants
 *
 * Ephemeral customer keypairs are persisted to scripts/.seed-state.json so
 * re-runs reuse them. Fund the dev key first (a few SOL).
 *
 * Run:  RPC_URL="https://devnet.helius-rpc.com/?api-key=…" npx tsx scripts/seed-master.ts
 */
import { createHash } from 'node:crypto'
import { existsSync, readFileSync, writeFileSync } from 'node:fs'
import { homedir } from 'node:os'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'
import {
  ASSOCIATED_TOKEN_PROGRAM_ID as ATOKEN,
  createAssociatedTokenAccountIdempotentInstruction,
  createTransferCheckedInstruction,
  getAssociatedTokenAddressSync,
  TOKEN_2022_PROGRAM_ID as T22,
} from '@solana/spl-token'
import {
  ComputeBudgetProgram,
  Connection,
  Keypair,
  PublicKey,
  SystemProgram,
  Transaction,
  type TransactionInstruction,
  sendAndConfirmTransaction,
} from '@solana/web3.js'

// ── program ids (deployed v2) ────────────────────────────────────────────────
const VESTA_CORE = new PublicKey('gaMq6BpH1aqC8ZCYtAxwZBjTa9AnfdWvYwURG6L4LDz')
const ARGUS = new PublicKey('9zJEWrk47z1ACT3ySMwzmUrMsQzFC8afBSFcsCzsz3rx')
const AEGIS = new PublicKey('AcCdMQC1rj4KukjhFzf4S8metEAXpnt9gzvMThsu15e1')
const RPC = process.env.RPC_URL ?? 'https://api.devnet.solana.com'
const PHANTOM = new PublicKey('ETasZGB4CX2Nbv3H2L3AKuMqWHdQrq8vhWkzJ4T6kLhN')
const DECIMALS = 2
const STATE_FILE = fileURLToPath(new URL('.seed-state.json', import.meta.url))

// ── encoding helpers (mirror vesta-ui/src/lib/vesta/ixns.ts) ─────────────────
const enc = new TextEncoder()
const disc = (n: string) => createHash('sha256').update(`global:${n}`).digest().subarray(0, 8)
const u64 = (n: bigint) => {
  const b = Buffer.alloc(8)
  b.writeBigUInt64LE(n)
  return b
}
const i64 = (n: bigint) => {
  const b = Buffer.alloc(8)
  b.writeBigInt64LE(n)
  return b
}
const u32 = (n: number) => {
  const b = Buffer.alloc(4)
  b.writeUInt32LE(n)
  return b
}
const u16 = (n: number) => {
  const b = Buffer.alloc(2)
  b.writeUInt16LE(n)
  return b
}
const i16 = (n: number) => {
  const b = Buffer.alloc(2)
  b.writeInt16LE(n)
  return b
}
const u8 = (n: number) => Buffer.from([n])
const bstr = (s: string) => {
  const body = enc.encode(s)
  return Buffer.concat([u32(body.length), Buffer.from(body)])
}
const meta = (pubkey: PublicKey, s: boolean, w: boolean) => ({ pubkey, isSigner: s, isWritable: w })
const link = (sig: string) => `https://explorer.solana.com/tx/${sig}?cluster=devnet`
const acct = (a: PublicKey) => `https://explorer.solana.com/address/${a.toBase58()}?cluster=devnet`

const corePda = (seeds: (Buffer | Uint8Array)[]) =>
  PublicKey.findProgramAddressSync(seeds, VESTA_CORE)[0]
const argusPda = (seeds: (Buffer | Uint8Array)[]) => PublicKey.findProgramAddressSync(seeds, ARGUS)[0]
const aegisPda = (seeds: (Buffer | Uint8Array)[]) => PublicKey.findProgramAddressSync(seeds, AEGIS)[0]
const ata = (mint: PublicKey, owner: PublicKey) =>
  getAssociatedTokenAddressSync(mint, owner, true, T22)

const pdas = {
  config: () => corePda([enc.encode('config')]),
  merchant: (auth: PublicKey, id: bigint) => corePda([enc.encode('merchant'), auth.toBuffer(), u64(id)]),
  mint: (merchant: PublicKey) => corePda([enc.encode('mint'), merchant.toBuffer()]),
  customer: (merchant: PublicKey, wallet: PublicKey) =>
    corePda([enc.encode('customer'), merchant.toBuffer(), wallet.toBuffer()]),
  offer: (merchant: PublicKey, id: bigint) => corePda([enc.encode('offer'), merchant.toBuffer(), u64(id)]),
  campaign: (merchant: PublicKey, id: bigint) =>
    corePda([enc.encode('campaign'), merchant.toBuffer(), u64(id)]),
  cprogress: (campaign: PublicKey, customer: PublicKey) =>
    corePda([enc.encode('cprogress'), campaign.toBuffer(), customer.toBuffer()]),
  achievement: (merchant: PublicKey, id: bigint) =>
    corePda([enc.encode('achieve'), merchant.toBuffer(), u64(id)]),
  badge: (achievement: PublicKey, customer: PublicKey) =>
    corePda([enc.encode('badge'), achievement.toBuffer(), customer.toBuffer()]),
  kleos: (achievement: PublicKey, customer: PublicKey) =>
    corePda([enc.encode('kleos'), achievement.toBuffer(), customer.toBuffer()]),
  receipt: (offer: PublicKey, customer: PublicKey, idx: number) =>
    corePda([enc.encode('receipt'), offer.toBuffer(), customer.toBuffer(), u32(idx)]),
  alliance: (creator: PublicKey, id: bigint) =>
    corePda([enc.encode('alliance'), creator.toBuffer(), u64(id)]),
  member: (alliance: PublicKey, merchant: PublicKey) =>
    corePda([enc.encode('member'), alliance.toBuffer(), merchant.toBuffer()]),
  guard: (mint: PublicKey) => argusPda([enc.encode('guard'), mint.toBuffer()]),
  wstate: (mint: PublicKey, owner: PublicKey) =>
    argusPda([enc.encode('wstate'), mint.toBuffer(), owner.toBuffer()]),
  entry: (mint: PublicKey, target: PublicKey) =>
    argusPda([enc.encode('entry'), mint.toBuffer(), target.toBuffer()]),
  eaml: (mint: PublicKey) => argusPda([enc.encode('extra-account-metas'), mint.toBuffer()]),
  issuer: (auth: PublicKey, id: bigint) => aegisPda([enc.encode('issuer'), auth.toBuffer(), u64(id)]),
  attestation: (issuer: PublicKey, subject: PublicKey, schemaId: bigint) =>
    aegisPda([
      enc.encode('attestation'),
      issuer.toBuffer(),
      subject.toBuffer(),
      Buffer.from(new BigUint64Array([schemaId]).buffer),
    ]),
  cap: (mint: PublicKey, subject: PublicKey) =>
    argusPda([enc.encode('cap'), mint.toBuffer(), subject.toBuffer()]),
}

// argus transfer-hook extra accounts (ExtraAccountMetaList order).
function argusExtras(mint: PublicKey, sourceOwner: PublicKey, destOwner: PublicKey) {
  return [
    meta(pdas.guard(mint), false, false),
    meta(pdas.wstate(mint, sourceOwner), false, true),
    meta(destOwner, false, false),
    meta(pdas.entry(mint, destOwner), false, false),
    meta(pdas.cap(mint, destOwner), false, false),
    meta(ARGUS, false, false),
    meta(pdas.eaml(mint), false, false),
  ]
}

// ── demo fixtures ────────────────────────────────────────────────────────────
const MERCHANTS = [
  { id: 10n, name: 'Kavarna Roasters', symbol: 'KAV', decayBps: -2000, category: 1, verify: true },
  { id: 11n, name: 'Duomo Books', symbol: 'DUOMO', decayBps: -1500, category: 2, verify: true },
  { id: 12n, name: 'Aegean Air Miles', symbol: 'AERO', decayBps: -800, category: 5, verify: false },
  { id: 13n, name: 'Palestra Fitness', symbol: 'FLEX', decayBps: -3000, category: 3, verify: false },
  { id: 14n, name: 'Cinema Vesuvio', symbol: 'VESUV', decayBps: -1200, category: 4, verify: false },
]
const CAMPAIGN_KIND = { MULTIPLIER: 0, FLAT_BONUS: 1, QUEST: 2 }
const GUARD_FLAG_BLOCK_PROGRAM = 1
const DEFAULT_GIFT_CAP = 50_000n // 500.00 pts
const CUSTOMER_COUNT = 6

async function main() {
  const connection = new Connection(RPC, 'confirmed')
  const dev = Keypair.fromSecretKey(
    Uint8Array.from(JSON.parse(readFileSync(join(homedir(), '.config/solana/id.json'), 'utf8'))),
  )
  const authority = dev.publicKey
  const budget = ComputeBudgetProgram.setComputeUnitLimit({ units: 600_000 })

  const report: string[] = []
  const ok: string[] = []
  const fail: string[] = []

  const send = async (label: string, ixns: TransactionInstruction[], signers: Keypair[] = [dev]) => {
    try {
      const tx = new Transaction().add(budget, ...ixns)
      const sig = await sendAndConfirmTransaction(connection, tx, signers, {
        commitment: 'confirmed',
      })
      ok.push(label)
      console.log(`  ✓ ${label}\n    ${link(sig)}`)
      return sig
    } catch (e) {
      const msg = e instanceof Error ? e.message.split('\n')[0] : String(e)
      fail.push(`${label}: ${msg}`)
      console.log(`  ✗ ${label}\n    ${msg}`)
      return null
    }
  }
  const exists = async (pk: PublicKey) => (await connection.getAccountInfo(pk)) !== null
  const ensure = async (label: string, pk: PublicKey, ixns: TransactionInstruction[], signers?: Keypair[]) => {
    if (await exists(pk)) {
      console.log(`  · ${label} (exists, skipped)`)
      return
    }
    await send(label, ixns, signers)
  }

  console.log(`\nVESTA master seed → ${RPC}`)
  console.log(`authority : ${authority.toBase58()}`)
  const bal = await connection.getBalance(authority)
  console.log(`balance   : ${(bal / 1e9).toFixed(3)} SOL`)
  if (bal < 0.3e9) console.log('  ! low balance — fund the dev key before seeding')

  // ── customers ──────────────────────────────────────────────────────────────
  const customers = loadOrCreateCustomers()
  console.log(`\ncustomers : ${customers.length} ephemeral + your Phantom wallet`)

  // Fund ephemeral customers so they can sign redeems/gifts.
  const fundIxns: TransactionInstruction[] = []
  for (const c of customers) {
    const cbal = await connection.getBalance(c.publicKey)
    if (cbal < 0.02e9)
      fundIxns.push(
        SystemProgram.transfer({ fromPubkey: authority, toPubkey: c.publicKey, lamports: 0.03e9 }),
      )
  }
  if (fundIxns.length > 0) {
    // batch in groups of 8 transfers per tx
    for (let i = 0; i < fundIxns.length; i += 8) {
      await send(`fund customers [${i}..${i + 8})`, fundIxns.slice(i, i + 8))
    }
  }

  // ── config ───────────────────────────────────────────────────────────────
  console.log('\n[1/9] protocol config')
  await ensure('init_config', pdas.config(), [
    {
      programId: VESTA_CORE,
      keys: [meta(authority, true, true), meta(pdas.config(), false, true), meta(SystemProgram.programId, false, false)],
      data: disc('init_config'),
    } as TransactionInstruction,
  ])

  const config = pdas.config()
  const today = Math.floor(Date.now() / 1000 / 86_400)
  const now = BigInt(Math.floor(Date.now() / 1000))

  // ── merchants + guards + offers + campaigns + achievements ──────────────────
  console.log('\n[2/9] merchants')
  const built: { m: (typeof MERCHANTS)[number]; merchant: PublicKey; mint: PublicKey }[] = []
  for (const m of MERCHANTS) {
    const merchant = pdas.merchant(authority, m.id)
    const mint = pdas.mint(merchant)
    built.push({ m, merchant, mint })
    await ensure(`register_merchant "${m.name}"`, merchant, [
      {
        programId: VESTA_CORE,
        keys: [
          meta(authority, true, true),
          meta(merchant, false, true),
          meta(mint, false, true),
          meta(ata(mint, authority), false, true),
          meta(config, false, false),
          meta(T22, false, false),
          meta(ATOKEN, false, false),
          meta(SystemProgram.programId, false, false),
        ],
        data: Buffer.concat([
          disc('register_merchant'),
          u64(m.id),
          bstr(m.name),
          bstr(m.symbol),
          bstr('https://dev-vesta.netlify.app/points.json'),
          i16(m.decayBps),
          u64(100n),
          u8(DECIMALS),
        ]),
      } as TransactionInstruction,
    ])

    // brand profile (category)
    await send(`update_merchant_profile "${m.name}"`, [
      {
        programId: VESTA_CORE,
        keys: [meta(authority, true, false), meta(merchant, false, true)],
        data: Buffer.concat([disc('update_merchant_profile'), u8(m.category), bstr('https://dev-vesta.netlify.app/brand.json')]),
      } as TransactionInstruction,
    ])

    // argus guard
    await ensure(`initialize_transfer_guard "${m.name}"`, pdas.guard(mint), [
      {
        programId: ARGUS,
        keys: [
          meta(authority, true, true),
          meta(merchant, false, false),
          meta(mint, false, false),
          meta(pdas.guard(mint), false, true),
          meta(pdas.eaml(mint), false, true),
          meta(SystemProgram.programId, false, false),
        ],
        data: Buffer.concat([
          disc('initialize_transfer_guard'),
          u16(GUARD_FLAG_BLOCK_PROGRAM),
          u64(DEFAULT_GIFT_CAP),
          u64(0n), // per_tx_cap
          u64(0n), // max_wallet_balance
          u16(0), // transfers_per_day_cap
          u32(0), // cooldown_secs
          PublicKey.default.toBuffer(), // aegis_program (attestation unused)
          PublicKey.default.toBuffer(), // policy
          PublicKey.default.toBuffer(), // attestation_issuer
          u64(0n), // attestation_schema
          i64(0n), // capability_ttl_secs (protocol default)
        ]),
      } as TransactionInstruction,
    ])

    // two offers
    for (const [oid, price, supply] of [
      [1n, 500n, 100],
      [2n, 1_500n, 50],
    ] as [bigint, bigint, number][]) {
      await ensure(`create_offer "${m.name}" #${oid}`, pdas.offer(merchant, oid), [
        {
          programId: VESTA_CORE,
          keys: [
            meta(authority, true, true),
            meta(merchant, false, false),
            meta(pdas.offer(merchant, oid), false, true),
            meta(config, false, false),
            meta(SystemProgram.programId, false, false),
          ],
          data: Buffer.concat([disc('create_offer'), u64(oid), u64(price * 100n), u32(supply)]),
        } as TransactionInstruction,
      ])
    }

    // one campaign (rotate kinds across merchants)
    const kind = Number(m.id) % 3
    const cid = 1n
    await ensure(`create_campaign "${m.name}"`, pdas.campaign(merchant, cid), [
      {
        programId: VESTA_CORE,
        keys: [
          meta(authority, true, true),
          meta(merchant, false, false),
          meta(pdas.campaign(merchant, cid), false, true),
          meta(config, false, false),
          meta(SystemProgram.programId, false, false),
        ],
        data: Buffer.concat([
          disc('create_campaign'),
          u64(cid),
          u8(kind),
          u16(kind === CAMPAIGN_KIND.MULTIPLIER ? 15_000 : 0), // 1.5x
          u64(kind === CAMPAIGN_KIND.FLAT_BONUS ? 5_000n : 0n),
          u16(kind === CAMPAIGN_KIND.QUEST ? 5 : 0),
          u64(kind === CAMPAIGN_KIND.QUEST ? 10_000n : 0n),
          u64(0n),
          u8(0),
          u64(0n),
          u64(0n),
          i64(now - 3_600n),
          i64(now + 30n * 86_400n),
          bstr(`${m.symbol} Launch`),
        ]),
      } as TransactionInstruction,
    ])

    // one achievement
    const aid = 1n
    await ensure(`create_achievement "${m.name}"`, pdas.achievement(merchant, aid), [
      {
        programId: VESTA_CORE,
        keys: [
          meta(authority, true, true),
          meta(merchant, false, false),
          meta(pdas.achievement(merchant, aid), false, true),
          meta(config, false, false),
          meta(SystemProgram.programId, false, false),
        ],
        data: Buffer.concat([
          disc('create_achievement'),
          u64(aid),
          bstr('Founding Patron'),
          bstr('https://dev-vesta.netlify.app/badge.json'),
          u64(3_000n * 100n),
        ]),
      } as TransactionInstruction,
    ])
  }

  // ── admin: verify a couple of merchants ─────────────────────────────────────
  console.log('\n[3/9] admin verification')
  for (const b of built.filter((x) => x.m.verify)) {
    await send(`verify_merchant "${b.m.name}"`, [
      {
        programId: VESTA_CORE,
        keys: [meta(authority, true, false), meta(b.merchant, false, true), meta(config, false, false)],
        data: Buffer.concat([disc('verify_merchant'), u8(1)]),
      } as TransactionInstruction,
    ])
  }

  // ── alliance ────────────────────────────────────────────────────────────────
  console.log('\n[4/9] alliance')
  const allianceId = 0n
  const alliance = pdas.alliance(authority, allianceId)
  await ensure('create_alliance "Koinon"', alliance, [
    {
      programId: VESTA_CORE,
      keys: [
        meta(authority, true, true),
        meta(alliance, false, true),
        meta(config, false, false),
        meta(SystemProgram.programId, false, false),
      ],
      data: Buffer.concat([disc('create_alliance'), u64(allianceId), bstr('Koinon')]),
    } as TransactionInstruction,
  ])
  for (const b of built.slice(0, 3)) {
    await ensure(`join_alliance "${b.m.name}"`, pdas.member(alliance, b.merchant), [
      {
        programId: VESTA_CORE,
        keys: [
          meta(authority, true, true),
          meta(authority, false, false), // self-join: alliance authority not a separate signer
          meta(b.merchant, false, true),
          meta(alliance, false, true),
          meta(pdas.member(alliance, b.merchant), false, true),
          meta(config, false, false),
          meta(SystemProgram.programId, false, false),
        ],
        data: Buffer.concat([disc('join_alliance'), u32(10_000), u64(1_000_000n)]),
      } as TransactionInstruction,
    ])
  }

  // ── aegis issuer + attestations ─────────────────────────────────────────────
  console.log('\n[5/9] aegis attestations')
  const issuer = pdas.issuer(authority, 1n)
  await ensure('init_issuer "VESTA Geo Oracle"', issuer, [
    {
      programId: AEGIS,
      keys: [meta(authority, true, true), meta(issuer, false, true), meta(SystemProgram.programId, false, false)],
      data: Buffer.concat([disc('init_issuer'), u64(1n), bstr('VESTA Geo Oracle')]),
    } as TransactionInstruction,
  ])
  const subjects = [PHANTOM, ...customers.slice(0, 3).map((c) => c.publicKey)]
  for (const [i, subject] of subjects.entries()) {
    const att = pdas.attestation(issuer, subject, 1n /* REGION */)
    await ensure(`issue_attestation ${subject.toBase58().slice(0, 6)}…`, att, [
      {
        programId: AEGIS,
        keys: [
          meta(authority, true, true),
          meta(issuer, false, true),
          meta(att, false, true),
          meta(SystemProgram.programId, false, false),
        ],
        data: Buffer.concat([
          disc('issue_attestation'),
          subject.toBuffer(),
          u64(1n), // schema: REGION
          Buffer.alloc(32, 2 + i), // commitment (demo: deterministic filler)
          Buffer.alloc(32, 0), // attr_root
          i64(0n), // valid_from
          i64(0n), // expires_at (0 = no expiry)
        ]),
      } as TransactionInstruction,
    ])
  }

  // ── issue points (dev-signed, gasless for customers) ────────────────────────
  console.log('\n[6/9] issue points')
  const allCustomers = [PHANTOM, ...customers.map((c) => c.publicKey)]
  // Spread earns across merchants & customers; a few via campaign.
  for (const [bi, b] of built.entries()) {
    const campaign = pdas.campaign(b.merchant, 1n)
    // three customers per merchant, rotating
    for (let k = 0; k < 3; k++) {
      const customer = allCustomers[(bi + k) % allCustomers.length]
      if (!customer) continue
      const amount = BigInt(1_000 + k * 750)
      const useCampaign = k === 0
      const profile = pdas.customer(b.merchant, customer)
      const custAta = ata(b.mint, customer)
      const keys = useCampaign
        ? [
            meta(authority, true, true),
            meta(b.merchant, false, true),
            meta(customer, false, false),
            meta(profile, false, true),
            meta(campaign, false, true),
            meta(pdas.cprogress(campaign, customer), false, true),
            meta(b.mint, false, true),
            meta(custAta, false, true),
            meta(config, false, false),
            meta(T22, false, false),
            meta(ATOKEN, false, false),
            meta(SystemProgram.programId, false, false),
          ]
        : [
            meta(authority, true, true),
            meta(b.merchant, false, true),
            meta(customer, false, false),
            meta(profile, false, true),
            meta(b.mint, false, true),
            meta(custAta, false, true),
            meta(config, false, false),
            meta(VESTA_CORE, false, false), // merchant_segments: None
            meta(VESTA_CORE, false, false), // customer_eligibility: None
            meta(T22, false, false),
            meta(ATOKEN, false, false),
            meta(SystemProgram.programId, false, false),
          ]
      await send(`earn ${amount} → ${customer.toBase58().slice(0, 4)}… @ ${b.m.symbol}${useCampaign ? ' (campaign)' : ''}`, [
        {
          programId: VESTA_CORE,
          keys,
          data: Buffer.concat([
            disc(useCampaign ? 'earn_points_campaign' : 'earn_points'),
            u64(amount),
            u32(today),
          ]),
        } as TransactionInstruction,
      ])
    }
  }

  // ── redeem an offer (customer-signed) ───────────────────────────────────────
  console.log('\n[7/9] redemption + gift (argus)')
  const c0 = customers[0]
  const b0 = built[0]
  if (c0 && b0) {
    // ensure c0 holds enough at b0: earn 2,000 first
    await send(`earn 2000 → ${c0.publicKey.toBase58().slice(0, 4)}… @ ${b0.m.symbol}`, [
      {
        programId: VESTA_CORE,
        keys: [
          meta(authority, true, true),
          meta(b0.merchant, false, true),
          meta(c0.publicKey, false, false),
          meta(pdas.customer(b0.merchant, c0.publicKey), false, true),
          meta(b0.mint, false, true),
          meta(ata(b0.mint, c0.publicKey), false, true),
          meta(config, false, false),
          meta(VESTA_CORE, false, false), // merchant_segments: None
          meta(VESTA_CORE, false, false), // customer_eligibility: None
          meta(T22, false, false),
          meta(ATOKEN, false, false),
          meta(SystemProgram.programId, false, false),
        ],
        data: Buffer.concat([disc('earn_points'), u64(2_000n), u32(today)]),
      } as TransactionInstruction,
    ])
    const offer = pdas.offer(b0.merchant, 1n)
    await send(`redeem_offer #1 by ${c0.publicKey.toBase58().slice(0, 4)}…`, [
      {
        programId: VESTA_CORE,
        keys: [
          meta(c0.publicKey, true, true),
          meta(b0.merchant, false, true),
          meta(offer, false, true),
          meta(pdas.customer(b0.merchant, c0.publicKey), false, true),
          meta(pdas.receipt(offer, c0.publicKey, 0), false, true),
          meta(b0.mint, false, true),
          meta(ata(b0.mint, c0.publicKey), false, true),
          meta(config, false, false),
          meta(VESTA_CORE, false, false), // merchant_segments: None
          meta(VESTA_CORE, false, false), // customer_eligibility: None
          meta(T22, false, false),
          meta(ATOKEN, false, false),
          meta(SystemProgram.programId, false, false),
        ],
        data: Buffer.concat([disc('redeem_offer'), u64(500n * 100n * 2n)]),
      } as TransactionInstruction,
    ], [c0])

    // guarded gift: c0 → c1 through argus
    const c1 = customers[1]
    if (c1) {
      const mint = b0.mint
      const transfer = createTransferCheckedInstruction(
        ata(mint, c0.publicKey),
        mint,
        ata(mint, c1.publicKey),
        c0.publicKey,
        100n * 100n,
        DECIMALS,
        [],
        T22,
      )
      transfer.keys.push(...argusExtras(mint, c0.publicKey, c1.publicKey))
      await send(`gift 100 pts ${c0.publicKey.toBase58().slice(0, 4)}… → ${c1.publicKey.toBase58().slice(0, 4)}… (argus)`, [
        // open sender wallet-state (idempotent-ish; skip if exists)
        ...((await exists(pdas.wstate(mint, c0.publicKey)))
          ? []
          : [
              {
                programId: ARGUS,
                keys: [
                  meta(c0.publicKey, true, true),
                  meta(mint, false, false),
                  meta(pdas.wstate(mint, c0.publicKey), false, true),
                  meta(SystemProgram.programId, false, false),
                ],
                data: disc('open_wallet_state'),
              } as TransactionInstruction,
            ]),
        createAssociatedTokenAccountIdempotentInstruction(c0.publicKey, ata(mint, c1.publicKey), c1.publicKey, mint, T22),
        transfer,
      ], [c0])
    }
  }

  // ── clawback (merchant delegate, through argus) ─────────────────────────────
  console.log('\n[8/9] clawback')
  if (b0 && customers[2]) {
    const victim = customers[2].publicKey
    // seed some balance to claw
    await send(`earn 1500 → ${victim.toBase58().slice(0, 4)}… @ ${b0.m.symbol}`, [
      {
        programId: VESTA_CORE,
        keys: [
          meta(authority, true, true),
          meta(b0.merchant, false, true),
          meta(victim, false, false),
          meta(pdas.customer(b0.merchant, victim), false, true),
          meta(b0.mint, false, true),
          meta(ata(b0.mint, victim), false, true),
          meta(config, false, false),
          meta(VESTA_CORE, false, false), // merchant_segments: None
          meta(VESTA_CORE, false, false), // customer_eligibility: None
          meta(T22, false, false),
          meta(ATOKEN, false, false),
          meta(SystemProgram.programId, false, false),
        ],
        data: Buffer.concat([disc('earn_points'), u64(1_500n), u32(today)]),
      } as TransactionInstruction,
    ])
    await send(`clawback 200 pts from ${victim.toBase58().slice(0, 4)}…`, [
      {
        programId: VESTA_CORE,
        keys: [
          meta(authority, true, true),
          meta(b0.merchant, false, true),
          meta(victim, false, false),
          meta(pdas.customer(b0.merchant, victim), false, true),
          meta(ata(b0.mint, victim), false, true),
          meta(ata(b0.mint, authority), false, true),
          meta(b0.mint, false, true),
          meta(config, false, false),
          meta(T22, false, false),
          meta(SystemProgram.programId, false, false),
          ...argusExtras(b0.mint, victim, authority),
        ],
        data: Buffer.concat([disc('clawback'), u64(200n * 100n), u16(1)]),
      } as TransactionInstruction,
    ])
  }

  // ── verification pass ───────────────────────────────────────────────────────
  console.log('\n[9/9] verification')
  const checks: [string, PublicKey][] = [
    ['config', config],
    ...built.map((b) => [`merchant ${b.m.symbol}`, b.merchant] as [string, PublicKey]),
    ...built.map((b) => [`guard ${b.m.symbol}`, pdas.guard(b.mint)] as [string, PublicKey]),
    ['alliance', alliance],
    ['issuer', issuer],
  ]
  for (const [label, pk] of checks) {
    const present = await exists(pk)
    console.log(`  ${present ? '✓' : '✗'} ${label} ${present ? '' : '(missing)'}`)
    if (present) report.push(label)
    else fail.push(`verify ${label} missing`)
  }

  // ── report ───────────────────────────────────────────────────────────────────
  console.log('\n───────────────────────────────────────────')
  console.log(`  seeded OK   : ${ok.length} transactions`)
  console.log(`  failures    : ${fail.length}`)
  console.log(`  accounts    : ${report.length}/${checks.length} present`)
  console.log('\n  Merchants:')
  for (const b of built) console.log(`    · ${b.m.name.padEnd(20)} ${acct(b.merchant)}`)
  console.log(`\n  Alliance   : ${acct(alliance)}`)
  console.log(`  Issuer     : ${acct(issuer)}`)
  console.log(`  Your wallet: ${acct(PHANTOM)}`)
  if (fail.length > 0) {
    console.log('\n  Failures:')
    for (const f of fail) console.log(`    ✗ ${f}`)
  }
  console.log('\nOpen the dashboard → Activity / Analytics to see it all live.\n')
}

// ── ephemeral customer persistence ────────────────────────────────────────────
function loadOrCreateCustomers(): Keypair[] {
  if (existsSync(STATE_FILE)) {
    const raw = JSON.parse(readFileSync(STATE_FILE, 'utf8')) as { customers: number[][] }
    return raw.customers.map((s) => Keypair.fromSecretKey(Uint8Array.from(s)))
  }
  const customers = Array.from({ length: CUSTOMER_COUNT }, () => Keypair.generate())
  writeFileSync(
    STATE_FILE,
    JSON.stringify({ customers: customers.map((c) => [...c.secretKey]) }, null, 2),
  )
  return customers
}

main().then(
  () => process.exit(0),
  (e) => {
    console.error(e)
    process.exit(1)
  },
)
