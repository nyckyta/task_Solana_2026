use anchor_lang::prelude::*;
use anchor_spl::{associated_token::AssociatedToken, token_interface::TokenInterface};
use resource_manager::{
    cpi::{accounts::MintResources, mint_resources},
    program::ResourceManager,
    GameConfig,
};

// TODO: Replace with actual program ID after `anchor build && anchor keys sync`
declare_id!("7wdTuafbSRJETjnBtoeSZy4fNWGA3VDWQxNYjSSB12zp");

/// Seconds a player must wait between search actions.
pub const SEARCH_COOLDOWN_SECS: i64 = 60;
/// Number of resources generated per search.
pub const RESOURCES_PER_SEARCH: usize = 3;
/// Total number of distinct resource types.
pub const RESOURCE_TYPE_COUNT: u8 = 6;

#[program]
pub mod search {
    use super::*;

    /// Register a new player. Creates the PlayerState PDA.
    pub fn register_player(ctx: Context<RegisterPlayer>) -> Result<()> {
        let state = &mut ctx.accounts.player_state;
        state.owner = ctx.accounts.player.key();
        state.last_search_timestamp = 0;
        state.bump = ctx.bumps.player_state;
        Ok(())
    }

    /// Search for resources.
    ///
    /// - Enforces a 60-second on-chain cooldown per player.
    /// - Derives 3 pseudo-random resource types using the SlotHashes sysvar.
    /// - Mints the resources via CPI to resource_manager.
    pub fn search_resources(ctx: Context<SearchResources>) -> Result<()> {
        let clock = Clock::get()?;
        let now = clock.unix_timestamp;

        let player_state = &ctx.accounts.player_state;
        let elapsed = now.saturating_sub(player_state.last_search_timestamp);
        require!(elapsed >= SEARCH_COOLDOWN_SECS, SearchError::CooldownNotElapsed);

        // Derive pseudo-random resource types from SlotHashes sysvar.
        let slot_hashes_data = ctx.accounts.slot_hashes.data.borrow();
        let resource_types = derive_random_resources(
            &slot_hashes_data,
            &ctx.accounts.player.key(),
            clock.slot,
        );
        drop(slot_hashes_data);

        let amounts = vec![1u64; RESOURCES_PER_SEARCH];

        // authority_seeds for the search program's authority PDA.
        let authority_seeds: &[&[u8]] = &[b"search_authority", &[ctx.bumps.search_authority]];
        let signer_seeds = &[authority_seeds];

        // Build remaining_accounts: [mint_0, player_ata_0, mint_1, player_ata_1, ...]
        // These are passed directly through ctx.remaining_accounts to the CPI.
        let cpi_ctx = CpiContext::new_with_signer(
            ctx.accounts.resource_manager_program.to_account_info(),
            MintResources {
                caller_authority: ctx.accounts.search_authority.to_account_info(),
                player: ctx.accounts.player.to_account_info(),
                game_config: ctx.accounts.resource_game_config.to_account_info(),
                resource_authority: ctx.accounts.resource_authority.to_account_info(),
                token_program: ctx.accounts.token_program.to_account_info(),
                associated_token_program: ctx.accounts.associated_token_program.to_account_info(),
                system_program: ctx.accounts.system_program.to_account_info(),
            },
            signer_seeds,
        )
        .with_remaining_accounts(ctx.remaining_accounts.to_vec());

        mint_resources(cpi_ctx, resource_types, amounts)?;

        // Update the player's last search timestamp.
        ctx.accounts.player_state.last_search_timestamp = now;
        Ok(())
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Derive 3 pseudo-random resource type indices (0..5) from SlotHashes data.
///
/// Uses the most recent slot hash as entropy, combined with the player pubkey and slot number.
/// This is NOT cryptographically secure but is acceptable for a game demo (same approach as
/// the reference EVM implementation using `block.prevrandao`).
fn derive_random_resources(
    slot_hashes_data: &[u8],
    player: &Pubkey,
    slot: u64,
) -> Vec<u8> {
    // SlotHashes sysvar layout: 8-byte length, then N × (8-byte slot + 32-byte hash)
    // We read the first hash (most recent slot).
    let hash_bytes: [u8; 32] = if slot_hashes_data.len() >= 8 + 40 {
        slot_hashes_data[16..48].try_into().unwrap_or([0u8; 32])
    } else {
        [0u8; 32]
    };

    let mut types = Vec::with_capacity(RESOURCES_PER_SEARCH);
    for i in 0u8..RESOURCES_PER_SEARCH as u8 {
        let mut seed = Vec::with_capacity(32 + 8 + 1 + 32);
        seed.extend_from_slice(&hash_bytes);
        seed.extend_from_slice(&slot.to_le_bytes());
        seed.push(i);
        seed.extend_from_slice(player.as_ref());

        let hash = anchor_lang::solana_program::keccak::hash(&seed);
        types.push(hash.0[0] % RESOURCE_TYPE_COUNT);
    }
    types
}

// ─── Accounts ────────────────────────────────────────────────────────────────

#[derive(Accounts)]
pub struct RegisterPlayer<'info> {
    #[account(mut)]
    pub player: Signer<'info>,

    #[account(
        init,
        payer = player,
        space = PlayerState::SIZE,
        seeds = [b"player_state", player.key().as_ref()],
        bump,
    )]
    pub player_state: Account<'info, PlayerState>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct SearchResources<'info> {
    #[account(mut)]
    pub player: Signer<'info>,

    #[account(
        mut,
        seeds = [b"player_state", player.key().as_ref()],
        bump = player_state.bump,
        constraint = player_state.owner == player.key() @ SearchError::NotPlayerOwner,
    )]
    pub player_state: Account<'info, PlayerState>,

    /// The search program's own authority PDA.
    /// This PDA signs the CPI to resource_manager as caller_authority.
    #[account(
        seeds = [b"search_authority"],
        bump,
    )]
    /// CHECK: PDA signer
    pub search_authority: UncheckedAccount<'info>,

    #[account(
        seeds = [b"game_config"],
        seeds::program = resource_manager_program.key(),
        bump = resource_game_config.bump,
    )]
    pub resource_game_config: Account<'info, GameConfig>,

    /// The resource_authority PDA owned by resource_manager.
    /// CHECK: PDA in the resource_manager program
    #[account(
        seeds = [b"resource_authority"],
        seeds::program = resource_manager_program.key(),
        bump,
    )]
    pub resource_authority: UncheckedAccount<'info>,

    pub resource_manager_program: Program<'info, ResourceManager>,
    pub token_program: Interface<'info, TokenInterface>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,

    /// CHECK: SlotHashes sysvar — used for pseudo-randomness
    #[account(address = anchor_lang::solana_program::sysvar::slot_hashes::id())]
    pub slot_hashes: UncheckedAccount<'info>,

    // remaining_accounts: [mint_0, player_ata_0, mint_1, player_ata_1, mint_2, player_ata_2]
}

// ─── State ───────────────────────────────────────────────────────────────────

/// Per-player on-chain state for the search program.
#[account]
pub struct PlayerState {
    pub owner: Pubkey,
    pub last_search_timestamp: i64,
    pub bump: u8,
}

impl PlayerState {
    pub const SIZE: usize = 8 + 32 + 8 + 1;
}

// ─── Errors ──────────────────────────────────────────────────────────────────

#[error_code]
pub enum SearchError {
    #[msg("Search cooldown has not elapsed (60 seconds required)")]
    CooldownNotElapsed,
    #[msg("Signer is not the owner of this player state account")]
    NotPlayerOwner,
}
