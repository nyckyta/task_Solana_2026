use anchor_lang::prelude::*;
use anchor_lang::solana_program::program::invoke_signed;
use anchor_spl::{
    associated_token::AssociatedToken,
    token_interface::{self, Mint, MintTo, TokenAccount, TokenInterface, Burn},
};
use spl_token_metadata_interface::instruction as metadata_ix;

// TODO: Replace with actual program ID after `anchor build && anchor keys sync`
declare_id!("CUH9R6iQXnhU78fn1FZWD9NQxweYtGbEH29y1PCr1ZLN");

/// Resource types available in the game.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq)]
pub enum ResourceType {
    Wood = 0,
    Iron = 1,
    Gold = 2,
    Leather = 3,
    Stone = 4,
    Diamond = 5,
}

impl ResourceType {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Wood),
            1 => Some(Self::Iron),
            2 => Some(Self::Gold),
            3 => Some(Self::Leather),
            4 => Some(Self::Stone),
            5 => Some(Self::Diamond),
            _ => None,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Wood => "Wood",
            Self::Iron => "Iron",
            Self::Gold => "Gold",
            Self::Leather => "Leather",
            Self::Stone => "Stone",
            Self::Diamond => "Diamond",
        }
    }

    pub fn symbol(&self) -> &'static str {
        match self {
            Self::Wood => "WOOD",
            Self::Iron => "IRON",
            Self::Gold => "GOLD",
            Self::Leather => "LEATHER",
            Self::Stone => "STONE",
            Self::Diamond => "DIAMOND",
        }
    }
}

#[program]
pub mod resource_manager {
    use super::*;

    /// Initialize the game configuration account.
    /// Must be called once by the admin before any other instructions.
    pub fn initialize(ctx: Context<Initialize>) -> Result<()> {
        let config = &mut ctx.accounts.game_config;
        config.admin = ctx.accounts.admin.key();
        config.bump = ctx.bumps.game_config;
        Ok(())
    }

    /// Initialize a single resource mint (SPL Token-2022 with MetadataPointer).
    /// Call once for each of the 6 resource types.
    pub fn init_resource_mint(
        ctx: Context<InitResourceMint>,
        resource_type: u8,
        uri: String,
    ) -> Result<()> {
        let rt = ResourceType::from_u8(resource_type)
            .ok_or(ResourceError::InvalidResourceType)?;

        let config = &mut ctx.accounts.game_config;
        require!(
            config.resource_mints[resource_type as usize] == Pubkey::default(),
            ResourceError::MintAlreadyInitialized
        );
        config.resource_mints[resource_type as usize] = ctx.accounts.mint.key();

        // Initialize token metadata embedded in the mint account via Token-2022 metadata interface.
        // Seeds for resource_authority PDA that signs the CPI.
        let seeds: &[&[u8]] = &[b"resource_authority", &[ctx.bumps.resource_authority]];
        let signer_seeds = &[seeds];

        let init_meta_ix = metadata_ix::initialize(
            &spl_token_2022::id(),
            &ctx.accounts.mint.key(),
            &ctx.accounts.resource_authority.key(), // update authority
            &ctx.accounts.mint.key(),               // mint (metadata = mint for MetadataPointer)
            &ctx.accounts.resource_authority.key(), // mint authority
            rt.name().to_string(),
            rt.symbol().to_string(),
            uri,
        );

        invoke_signed(
            &init_meta_ix,
            &[
                ctx.accounts.mint.to_account_info(),
                ctx.accounts.resource_authority.to_account_info(),
            ],
            signer_seeds,
        )?;

        Ok(())
    }

    /// Set the authorized program authority PDAs.
    /// search_authority  = PDA of the search program   (seeds: [b"search_authority"])
    /// crafting_authority = PDA of the crafting program (seeds: [b"crafting_authority"])
    pub fn set_authorities(
        ctx: Context<SetAuthorities>,
        search_authority: Pubkey,
        crafting_authority: Pubkey,
    ) -> Result<()> {
        let config = &mut ctx.accounts.game_config;
        config.search_program_authority = search_authority;
        config.crafting_program_authority = crafting_authority;
        Ok(())
    }

    /// Mint resources to a player. Only callable by the search program via CPI.
    ///
    /// remaining_accounts layout: [mint_0, player_ata_0, mint_1, player_ata_1, ...]
    pub fn mint_resources<'info>(
        ctx: Context<'_, '_, '_, 'info, MintResources<'info>>,
        resource_types: Vec<u8>,
        amounts: Vec<u64>,
    ) -> Result<()> {
        require!(
            resource_types.len() == amounts.len(),
            ResourceError::InvalidArgs
        );
        require!(
            ctx.remaining_accounts.len() == resource_types.len() * 2,
            ResourceError::InvalidArgs
        );
        require!(
            ctx.accounts.caller_authority.key()
                == ctx.accounts.game_config.search_program_authority,
            ResourceError::Unauthorized
        );

        let config = &ctx.accounts.game_config;
        let authority_seeds: &[&[u8]] =
            &[b"resource_authority", &[ctx.bumps.resource_authority]];
        let signer_seeds = &[authority_seeds];

        for (i, (&rt, &amount)) in resource_types.iter().zip(amounts.iter()).enumerate() {
            require!(rt < 6, ResourceError::InvalidResourceType);

            let mint_info = &ctx.remaining_accounts[i * 2];
            let ata_info = &ctx.remaining_accounts[i * 2 + 1];

            require!(
                mint_info.key() == config.resource_mints[rt as usize],
                ResourceError::InvalidMint
            );

            token_interface::mint_to(
                CpiContext::new_with_signer(
                    ctx.accounts.token_program.to_account_info(),
                    MintTo {
                        mint: mint_info.clone(),
                        to: ata_info.clone(),
                        authority: ctx.accounts.resource_authority.to_account_info(),
                    },
                    signer_seeds,
                ),
                amount,
            )?;
        }

        Ok(())
    }

    /// Burn resources from a player. Only callable by the crafting program via CPI.
    ///
    /// remaining_accounts layout: [mint_0, player_ata_0, mint_1, player_ata_1, ...]
    pub fn burn_resources<'info>(
        ctx: Context<'_, '_, '_, 'info, BurnResources<'info>>,
        resource_types: Vec<u8>,
        amounts: Vec<u64>,
    ) -> Result<()> {
        require!(
            resource_types.len() == amounts.len(),
            ResourceError::InvalidArgs
        );
        require!(
            ctx.remaining_accounts.len() == resource_types.len() * 2,
            ResourceError::InvalidArgs
        );
        require!(
            ctx.accounts.caller_authority.key()
                == ctx.accounts.game_config.crafting_program_authority,
            ResourceError::Unauthorized
        );

        let config = &ctx.accounts.game_config;

        for (i, (&rt, &amount)) in resource_types.iter().zip(amounts.iter()).enumerate() {
            require!(rt < 6, ResourceError::InvalidResourceType);

            let mint_info = &ctx.remaining_accounts[i * 2];
            let ata_info = &ctx.remaining_accounts[i * 2 + 1];

            require!(
                mint_info.key() == config.resource_mints[rt as usize],
                ResourceError::InvalidMint
            );

            // The player's ATA is the authority for their own token account.
            // The crafting program passes the player as a signer through its own CPI.
            token_interface::burn(
                CpiContext::new(
                    ctx.accounts.token_program.to_account_info(),
                    Burn {
                        mint: mint_info.clone(),
                        from: ata_info.clone(),
                        authority: ctx.accounts.player.to_account_info(),
                    },
                ),
                amount,
            )?;
        }

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
        space = GameConfig::SIZE,
        seeds = [b"game_config"],
        bump,
    )]
    pub game_config: Account<'info, GameConfig>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(resource_type: u8)]
pub struct InitResourceMint<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,

    #[account(
        mut,
        seeds = [b"game_config"],
        bump = game_config.bump,
        has_one = admin,
    )]
    pub game_config: Account<'info, GameConfig>,

    /// SPL Token-2022 mint with MetadataPointer extension (metadata_address = mint itself).
    #[account(
        init,
        payer = admin,
        mint::decimals = 0,
        mint::authority = resource_authority,
        mint::freeze_authority = resource_authority,
        mint::token_program = token_program,
        extensions::metadata_pointer::authority = resource_authority,
        extensions::metadata_pointer::metadata_address = mint,
    )]
    pub mint: Box<InterfaceAccount<'info, Mint>>,

    /// PDA that acts as mint/freeze authority for all resource mints.
    #[account(
        seeds = [b"resource_authority"],
        bump,
    )]
    /// CHECK: PDA signer — no lamports or data owned by this program
    pub resource_authority: UncheckedAccount<'info>,

    pub token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
pub struct SetAuthorities<'info> {
    pub admin: Signer<'info>,

    #[account(
        mut,
        seeds = [b"game_config"],
        bump = game_config.bump,
        has_one = admin,
    )]
    pub game_config: Account<'info, GameConfig>,
}

#[derive(Accounts)]
pub struct MintResources<'info> {
    /// The search program's authority PDA — must match game_config.search_program_authority.
    pub caller_authority: Signer<'info>,

    /// CHECK: Recipient of the minted tokens; validated per-mint in the instruction body.
    pub player: UncheckedAccount<'info>,

    #[account(
        seeds = [b"game_config"],
        bump = game_config.bump,
    )]
    pub game_config: Account<'info, GameConfig>,

    /// PDA that holds mint authority over all resource mints.
    #[account(
        seeds = [b"resource_authority"],
        bump,
    )]
    /// CHECK: PDA signer
    pub resource_authority: UncheckedAccount<'info>,

    pub token_program: Interface<'info, TokenInterface>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct BurnResources<'info> {
    /// The crafting program's authority PDA — must match game_config.crafting_program_authority.
    pub caller_authority: Signer<'info>,

    /// The player whose resources are being burned; must also sign (passed via CPI).
    pub player: Signer<'info>,

    #[account(
        seeds = [b"game_config"],
        bump = game_config.bump,
    )]
    pub game_config: Account<'info, GameConfig>,

    pub token_program: Interface<'info, TokenInterface>,
}

// ─── State ───────────────────────────────────────────────────────────────────

/// Global configuration for the resource manager.
#[account]
pub struct GameConfig {
    /// Admin that can set authorities and initialize mints.
    pub admin: Pubkey,
    /// One mint per resource type (index = ResourceType as u8).
    pub resource_mints: [Pubkey; 6],
    /// PDA of the search program, allowed to mint resources.
    pub search_program_authority: Pubkey,
    /// PDA of the crafting program, allowed to burn resources.
    pub crafting_program_authority: Pubkey,
    pub bump: u8,
}

impl GameConfig {
    pub const SIZE: usize = 8   // discriminator
        + 32                    // admin
        + 32 * 6               // resource_mints
        + 32                    // search_program_authority
        + 32                    // crafting_program_authority
        + 1;                    // bump
}

// ─── Errors ──────────────────────────────────────────────────────────────────

#[error_code]
pub enum ResourceError {
    #[msg("Invalid resource type — must be 0..5")]
    InvalidResourceType,
    #[msg("Mint for this resource type is already initialized")]
    MintAlreadyInitialized,
    #[msg("Caller is not an authorized program authority")]
    Unauthorized,
    #[msg("Provided mint does not match the stored resource mint")]
    InvalidMint,
    #[msg("resource_types and amounts must have equal length")]
    InvalidArgs,
}
