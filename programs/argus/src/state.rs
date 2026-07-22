use anchor_lang::prelude::*;

/// Per-mint transfer policy — the tunable heart of the guard (spec §2.1).
///
/// One account, read once in `execute`. The guard authority (the merchant PDA)
/// retunes fields via `configure_policy`; the trusted aegis issuer/program are
/// bound at init. Eligibility is no longer read from aegis by byte offset —
/// `execute` reads a cached `EligibilityCapability` (spec 09) minted off the hot
/// path by `refresh_eligibility`, which consumes aegis's `verify` interface.
#[account]
#[derive(InitSpace)]
pub struct GuardConfig {
    /// Layout version (Track B convention).
    pub version: u8,
    /// The hooked mint this config governs.
    pub mint: Pubkey,
    /// Who may retune policy — the vesta_core Merchant PDA (rotatable two-step).
    pub authority: Pubkey,
    /// Two-step authority rotation target.
    pub pending_authority: Option<Pubkey>,
    /// Always-allowed destination (rule 2): the merchant treasury ATA.
    pub treasury: Pubkey,
    /// aegis deployment this guard trusts (the `verify` program).
    pub aegis_program: Pubkey,
    /// aegis issuer whose credentials this guard trusts (spec §7).
    pub attestation_issuer: Pubkey,
    /// aegis `Policy` this guard enforces (spec 07). When set, `refresh_eligibility`
    /// consumes `verify_policy` — so the compliance rule (jurisdiction, schema,
    /// freshness) lives in aegis as data, editable with NO argus redeploy. When
    /// `default()`, the legacy single-credential `Present` check is used instead.
    pub policy: Pubkey,
    /// Bumped on any policy change; stamped into capabilities so a config change
    /// invalidates stale eligibility regardless of TTL (spec 09 §4.4).
    pub policy_epoch: u64,
    /// Per-mint peer-transfer freeze (rules 1–2 stay open — clawback/refunds).
    pub paused: bool,
    /// Policy bitset — see `crate::constants::flags`.
    pub flags: u16,
    /// Raw units a wallet may send per UTC day (0 = gifting off).
    pub daily_gift_cap: u64,
    /// Max raw units in a single peer transfer (0 = no per-tx limit).
    pub per_tx_cap: u64,
    /// Reject transfers that would push the destination over this (0 = off).
    pub max_wallet_balance: u64,
    /// Max peer transfers a wallet may make per UTC day (0 = no count limit).
    pub transfers_per_day_cap: u16,
    /// Minimum seconds between a wallet's peer transfers (0 = no cooldown).
    pub cooldown_secs: u32,
    /// Required aegis schema id (meaningful only with REQUIRE_ATTESTATION).
    pub attestation_schema: u64,
    /// Per-guard eligibility-capability TTL, seconds (0 = protocol default). A
    /// strict mint sets this low to shrink the aegis-revocation-latency window;
    /// `invalidate_capability` closes it immediately for a known revocation.
    pub capability_ttl_secs: i64,
    /// True once the mint adopts the governed policy lifecycle (spec 10 §4.1).
    /// While set, `configure_policy` (free-tier live mutation) is rejected — all
    /// policy changes must go through propose → approve → timelock → activate.
    pub governed: bool,
    /// Hash of the currently-active governed `PolicyVersion` (`[0;32]` for a
    /// free-tier mint). Denormalized here so `execute` can stamp every decision
    /// record with the exact deciding policy at zero hot-path cost (spec 10 §4.5).
    pub active_policy_hash: [u8; 32],
    pub bump: u8,
}

impl GuardConfig {
    /// Snapshot the governed (retunable) fields as a `PolicyDoc` — used to seed
    /// the genesis version when a mint adopts governance (spec 10 §4.1).
    pub fn as_policy_doc(&self) -> PolicyDoc {
        PolicyDoc {
            flags: self.flags,
            daily_gift_cap: self.daily_gift_cap,
            per_tx_cap: self.per_tx_cap,
            max_wallet_balance: self.max_wallet_balance,
            transfers_per_day_cap: self.transfers_per_day_cap,
            cooldown_secs: self.cooldown_secs,
            attestation_schema: self.attestation_schema,
            capability_ttl_secs: self.capability_ttl_secs,
            policy: self.policy,
        }
    }
}

/// Per-(mint, source-owner) velocity counters (spec §2.2). Deliberately
/// non-closable: closing and reopening would reset the daily counters, so the
/// locked rent is the anti-reset bond.
#[account]
#[derive(InitSpace)]
pub struct WalletPolicyState {
    /// UTC day the counters below belong to.
    pub day: u32,
    /// Raw units sent as peer transfers today.
    pub sent_today: u64,
    /// Peer-transfer count today.
    pub transfers_today: u16,
    /// Unix ts of the wallet's last peer transfer (cooldown reference).
    pub last_transfer_at: i64,
    pub bump: u8,
}

/// Allow/deny list membership marker (spec §2.4). Existence under the argus
/// program == membership; the account carries no payload beyond identity.
#[account]
#[derive(InitSpace)]
pub struct PolicyListEntry {
    pub mint: Pubkey,
    pub target: Pubkey,
    pub bump: u8,
}

/// Cached eligibility verdict for a subject (spec 09 §4.1). `refresh_eligibility`
/// pays aegis's `verify` CPI once, off the transfer path, and stamps the result
/// here; `execute` then does an O(1) freshness + bitmap read with NO CPI. A
/// missing/stale capability fails the transfer closed (client must refresh).
#[account]
#[derive(InitSpace)]
pub struct EligibilityCapability {
    pub version: u8,
    pub mint: Pubkey,
    pub subject: Pubkey,
    /// Bit `i` set = predicate `i` satisfied (see `crate::constants` bits).
    pub verdicts: u32,
    /// aegis deployment consulted when this capability was minted.
    pub aegis_program: Pubkey,
    /// `GuardConfig.policy_epoch` at mint time; must still match to be valid.
    pub policy_epoch: u64,
    pub issued_at: i64,
    /// Unix expiry; the capability is stale once `now >= expires_at`.
    pub expires_at: i64,
    pub bump: u8,
}

// ── Governance (spec 10) ─────────────────────────────────────────────────────

/// The tunable policy fields under governance — the content that a
/// `PolicyVersion` hashes and that `activate_policy` writes onto `GuardConfig`
/// (spec 10 §4.1). Deliberately the retunable subset: the aegis *program* and
/// trusted issuer are bound immutably at guard init and are NOT governed here.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, Default, InitSpace, PartialEq, Eq)]
pub struct PolicyDoc {
    pub flags: u16,
    pub daily_gift_cap: u64,
    pub per_tx_cap: u64,
    pub max_wallet_balance: u64,
    pub transfers_per_day_cap: u16,
    pub cooldown_secs: u32,
    pub attestation_schema: u64,
    pub capability_ttl_secs: i64,
    /// aegis `Policy` to enforce (`Pubkey::default()` = legacy `Present` check).
    pub policy: Pubkey,
}

impl PolicyDoc {
    /// Content address: sha256 over the borsh encoding. Two proposals with the
    /// same rules share a version account (idempotent, deduplicated).
    pub fn hash(&self) -> Result<[u8; 32]> {
        let mut bytes = Vec::new();
        self.serialize(&mut bytes)
            .map_err(|_| crate::error::GuardError::InvalidPolicy)?;
        Ok(solana_sha256_hasher::hashv(&[&bytes]).to_bytes())
    }
}

/// An immutable, content-addressed proposed/approved policy (spec 10 §4.1).
/// Seeds `["pver", mint, policy_hash]` — the hash IS the identity, so a version
/// can never be silently rewritten. Lifecycle metadata (author/approver/times)
/// is appended; the `doc` itself is frozen at propose time.
#[account]
#[derive(InitSpace)]
pub struct PolicyVersion {
    pub version: u8,
    pub mint: Pubkey,
    /// sha256 of `doc` — equals the PDA seed component.
    pub policy_hash: [u8; 32],
    pub doc: PolicyDoc,
    /// Role::Author key that proposed it.
    pub author: Pubkey,
    /// Role::Approver key that approved it (`default()` until approved).
    pub approver: Pubkey,
    pub proposed_at: i64,
    /// Unix ts of approval (`0` until approved) — the timelock reference.
    pub approved_at: i64,
    pub bump: u8,
}

/// Per-mint active-policy pointer + pending-change timelock (spec 10 §4.1, §5).
/// This is the governed replacement for silent live-mutation of `GuardConfig`.
#[account]
#[derive(InitSpace)]
pub struct PolicyPointer {
    pub version: u8,
    pub mint: Pubkey,
    /// Hash of the currently-active `PolicyVersion`.
    pub active_hash: [u8; 32],
    /// Hash of a proposed version awaiting approval/timelock (`[0;32]` = none).
    pub pending_hash: [u8; 32],
    /// Approval ts of the pending version (`0` = proposed-but-unapproved).
    pub pending_approved_at: i64,
    /// Seconds that must elapse after approval before activation.
    pub timelock_secs: i64,
    /// Configurable immutability (spec 10 §4.1): once pinned, no propose/activate;
    /// only the freeze (pause) authority stays alive.
    pub pinned: bool,
    pub bump: u8,
}

/// The separation-of-duties roles (spec 10 §4.2). Each maps to a single
/// authority key — which may itself be a multisig (e.g. a Squads PDA); argus
/// treats it as one signer, so multisig is transparent.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// Grants/revokes roles (itself the most privileged; timelocked changes).
    RoleAdmin,
    /// Proposes new policy versions.
    Author,
    /// Approves a proposed version (must differ from its author).
    Approver,
    /// Activates an approved version after the timelock; rolls back.
    Activator,
    /// Instant freeze-only (cannot change rules or unpause).
    PauseOperator,
    /// Anchors period decision statements.
    Reporter,
}

/// Per-mint role registry (spec 10 §4.2), seeds `["roles", mint]`. A mint that
/// never adopts governance never creates this; free-tier stays single-authority.
#[account]
#[derive(InitSpace)]
pub struct RoleRegistry {
    pub version: u8,
    pub mint: Pubkey,
    pub role_admin: Pubkey,
    pub author: Pubkey,
    pub approver: Pubkey,
    pub activator: Pubkey,
    pub pause_operator: Pubkey,
    pub reporter: Pubkey,
    /// A two-step, timelocked pending role change (spec 10 §4.2 "timelocked").
    /// `pending_after == 0` means no change is queued.
    pub pending_role: u8,
    pub pending_authority: Pubkey,
    pub pending_after: i64,
    pub bump: u8,
}

/// Tamper-evident anchor for a period's decision records (spec 10 §4.5), seeds
/// `["statement", mint, period_le]`. An off-chain indexer materializes the
/// canonical, reason-coded decisions for the period into a Merkle tree; the
/// Reporter role anchors its root here. `decision_count` makes the statement
/// **provably complete** — an omission changes the count and the root.
#[account]
#[derive(InitSpace)]
pub struct StatementCommitment {
    pub version: u8,
    pub mint: Pubkey,
    /// Period identifier (e.g. a UTC-day index) — also the PDA seed component.
    pub period: u64,
    pub merkle_root: [u8; 32],
    /// Number of decisions covered — completeness witness.
    pub decision_count: u64,
    pub reporter: Pubkey,
    pub anchored_at: i64,
    pub bump: u8,
}

impl RoleRegistry {
    /// The live authority for `role`.
    pub fn authority_for(&self, role: Role) -> Pubkey {
        match role {
            Role::RoleAdmin => self.role_admin,
            Role::Author => self.author,
            Role::Approver => self.approver,
            Role::Activator => self.activator,
            Role::PauseOperator => self.pause_operator,
            Role::Reporter => self.reporter,
        }
    }

    /// Overwrite the authority for `role`.
    pub fn set(&mut self, role: Role, authority: Pubkey) {
        match role {
            Role::RoleAdmin => self.role_admin = authority,
            Role::Author => self.author = authority,
            Role::Approver => self.approver = authority,
            Role::Activator => self.activator = authority,
            Role::PauseOperator => self.pause_operator = authority,
            Role::Reporter => self.reporter = authority,
        }
    }

    /// Assert `signer` holds `role`. Fail-closed: an unset (default) role
    /// authority matches nothing, so an unconfigured role can never be invoked.
    pub fn require(&self, role: Role, signer: Pubkey) -> Result<()> {
        let authority = self.authority_for(role);
        require!(
            authority != Pubkey::default() && authority == signer,
            crate::error::GuardError::RoleUnauthorized
        );
        Ok(())
    }
}

impl TryFrom<u8> for Role {
    type Error = Error;
    fn try_from(v: u8) -> Result<Self> {
        Ok(match v {
            0 => Role::RoleAdmin,
            1 => Role::Author,
            2 => Role::Approver,
            3 => Role::Activator,
            4 => Role::PauseOperator,
            5 => Role::Reporter,
            _ => return err!(crate::error::GuardError::InvalidRole),
        })
    }
}
