use anchor_lang::prelude::*;
use anchor_spl::{associated_token::AssociatedToken, token_interface::TokenInterface};
use item_nft::{
    cpi::{accounts::MintItem, mint_item},
    program::ItemNft,
    ItemConfig,
};
use resource_manager::{
    cpi::{accounts::BurnResources, burn_resources},
    program::ResourceManager,
    GameConfig as ResourceGameConfig,
};

// TODO: Replace with actual program ID after `anchor build && anchor keys sync`
declare_id!("GJftT2Kndy75ktGJtJu4ti6X65ovHeDqwb1U3BV4Ncfp");

/// Recipe defines the resources required to craft one item type.
struct Recipe {
    /// Parallel arrays: resource_type index and required amount.
    types: &'static [u8],
    amounts: &'static [u64],
}

/// Return the crafting recipe for a given item type.
///
/// Recipes match the EVM reference implementation exactly:
/// - KozakSable  (0): 3×Iron + 1×Wood + 1×Leather
/// - ElderStick  (1): 2×Wood + 1×Gold + 1×Diamond
/// - Armour      (2): 4×Leather + 2×Iron + 1×Gold
/// - Brace       (3): 4×Iron + 2×Gold + 2×Diamond
fn recipe_for(item_type: u8) -> Option<Recipe> {
    match item_type {
        // Iron=1, Wood=0, Leather=3
        0 => Some(Recipe { types: &[1, 0, 3], amounts: &[3, 1, 1] }),
        // Wood=0, Gold=2, Diamond=5
        1 => Some(Recipe { types: &[0, 2, 5], amounts: &[2, 1, 1] }),
        // Leather=3, Iron=1, Gold=2
        2 => Some(Recipe { types: &[3, 1, 2], amounts: &[4, 2, 1] }),
        // Iron=1, Gold=2, Diamond=5
        3 => Some(Recipe { types: &[1, 2, 5], amounts: &[4, 2, 2] }),
        _ => None,
    }
}

#[program]
pub mod crafting {
    use super::*;

    /// Initialize the crafting configuration PDA.
    pub fn initialize(ctx: Context<Initialize>) -> Result<()> {
        let config = &mut ctx.accounts.config;
        config.admin = ctx.accounts.admin.key();
        config.bump = ctx.bumps.config;
        config.crafting_authority_bump = ctx.bumps.crafting_authority;
        Ok(())
    }

    /// Craft an item by burning the required resources and minting an NFT.
    ///
    /// remaining_accounts layout (in recipe order):
    ///   [mint_0, player_ata_0, mint_1, player_ata_1, mint_2, player_ata_2,  -- resource mints/ATAs
    ///    item_mint, item_player_ata, metadata, master_edition,               -- NFT accounts
    ///    sysvar_instructions, token_metadata_program]
    pub fn craft_item<'info>(ctx: Context<'_, '_, '_, 'info, CraftItem<'info>>, item_type: u8) -> Result<()> {
        let recipe = recipe_for(item_type).ok_or(CraftingError::InvalidItemType)?;
        let n = recipe.types.len();

        require!(
            ctx.remaining_accounts.len() >= n * 2 + 4,
            CraftingError::InvalidRemainingAccounts
        );

        let authority_bump = ctx.accounts.config.crafting_authority_bump;
        let authority_seeds: &[&[u8]] = &[b"crafting_authority", &[authority_bump]];
        let signer_seeds = &[authority_seeds];

        // ── Step 1: Burn resources via CPI to resource_manager ───────────────
        let resource_remaining: Vec<AccountInfo> = ctx.remaining_accounts[..n * 2].to_vec();

        let burn_ctx = CpiContext::new_with_signer(
            ctx.accounts.resource_manager_program.to_account_info(),
            BurnResources {
                caller_authority: ctx.accounts.crafting_authority.to_account_info(),
                player: ctx.accounts.player.to_account_info(),
                game_config: ctx.accounts.resource_game_config.to_account_info(),
                token_program: ctx.accounts.token_program.to_account_info(),
            },
            signer_seeds,
        )
        .with_remaining_accounts(resource_remaining);

        burn_resources(burn_ctx, recipe.types.to_vec(), recipe.amounts.to_vec())?;

        // ── Step 2: Mint the NFT via CPI to item_nft ─────────────────────────
        let base = n * 2;
        let item_mint_info = ctx.remaining_accounts[base].clone();
        let player_ata_info = ctx.remaining_accounts[base + 1].clone();
        let metadata_info = ctx.remaining_accounts[base + 2].clone();
        let master_edition_info = ctx.remaining_accounts[base + 3].clone();
        let sysvar_instructions_info = ctx.remaining_accounts[base + 4].clone();
        let token_metadata_program_info = ctx.remaining_accounts[base + 5].clone();

        let mint_ctx = CpiContext::new_with_signer(
            ctx.accounts.item_nft_program.to_account_info(),
            MintItem {
                caller_authority: ctx.accounts.crafting_authority.to_account_info(),
                config: ctx.accounts.item_config.to_account_info(),
                item_authority: ctx.accounts.item_authority.to_account_info(),
                mint: item_mint_info,
                metadata: metadata_info,
                master_edition: master_edition_info,
                player: ctx.accounts.player.to_account_info(),
                player_ata: player_ata_info,
                item_metadata: ctx.remaining_accounts[base + 6].clone(),
                fee_payer: ctx.accounts.player.to_account_info(),
                token_program: ctx.accounts.spl_token_program.to_account_info(),
                associated_token_program: ctx.accounts.associated_token_program.to_account_info(),
                system_program: ctx.accounts.system_program.to_account_info(),
                sysvar_instructions: sysvar_instructions_info,
                token_metadata_program: token_metadata_program_info,
            },
            signer_seeds,
        );

        mint_item(mint_ctx, item_type)?;

        Ok(())
    }
}

// ─── Accounts ────────────────────────────────────────────────────────────────

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,

    #[account(
        init,
        payer = admin,
        space = CraftingConfig::SIZE,
        seeds = [b"crafting_config"],
        bump,
    )]
    pub config: Account<'info, CraftingConfig>,

    #[account(
        seeds = [b"crafting_authority"],
        bump,
    )]
    /// CHECK: PDA signer
    pub crafting_authority: UncheckedAccount<'info>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct CraftItem<'info> {
    #[account(mut)]
    pub player: Signer<'info>,

    #[account(
        seeds = [b"crafting_config"],
        bump = config.bump,
    )]
    pub config: Account<'info, CraftingConfig>,

    /// Crafting program's authority PDA — signs as caller_authority in both CPIs.
    #[account(
        seeds = [b"crafting_authority"],
        bump = config.crafting_authority_bump,
    )]
    /// CHECK: PDA signer
    pub crafting_authority: UncheckedAccount<'info>,

    // ── resource_manager accounts ──────────────────────────────────────────
    #[account(
        seeds = [b"game_config"],
        seeds::program = resource_manager_program.key(),
        bump = resource_game_config.bump,
    )]
    pub resource_game_config: Account<'info, ResourceGameConfig>,

    pub resource_manager_program: Program<'info, ResourceManager>,

    // ── item_nft accounts ─────────────────────────────────────────────────
    #[account(
        seeds = [b"item_config"],
        seeds::program = item_nft_program.key(),
        bump = item_config.bump,
    )]
    pub item_config: Account<'info, ItemConfig>,

    /// CHECK: item_authority PDA in item_nft program
    #[account(
        seeds = [b"item_authority"],
        seeds::program = item_nft_program.key(),
        bump = item_config.item_authority_bump,
    )]
    pub item_authority: UncheckedAccount<'info>,

    pub item_nft_program: Program<'info, ItemNft>,

    // ── shared ────────────────────────────────────────────────────────────
    pub token_program: Interface<'info, TokenInterface>,
    /// CHECK: SPL Token program (legacy) — needed for item_nft Metaplex CPI
    #[account(address = anchor_spl::token::ID)]
    pub spl_token_program: UncheckedAccount<'info>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

// ─── State ───────────────────────────────────────────────────────────────────

#[account]
pub struct CraftingConfig {
    pub admin: Pubkey,
    pub bump: u8,
    pub crafting_authority_bump: u8,
}

impl CraftingConfig {
    pub const SIZE: usize = 8 + 32 + 1 + 1;
}

// ─── Errors ──────────────────────────────────────────────────────────────────

#[error_code]
pub enum CraftingError {
    #[msg("Invalid item type — must be 0..3")]
    InvalidItemType,
    #[msg("remaining_accounts does not match the expected layout for this recipe")]
    InvalidRemainingAccounts,
}
