use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{self, Mint, MintTo, Token, TokenAccount},
};
use mpl_token_metadata::{
    instructions::{BurnV1CpiBuilder, CreateV1CpiBuilder, MintV1CpiBuilder},
    types::{PrintSupply, TokenStandard},
};

// TODO: Replace with actual program ID after `anchor build && anchor keys sync`
declare_id!("7h34th7JokUSqF7ewdC6E8rU4R7qxQBtmX2naM5tgqkX");

/// Item types that can be crafted in the game.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum ItemType {
    /// Козацька шабля — 3×Iron + 1×Wood + 1×Leather
    KozakSable = 0,
    /// Посох старійшини — 2×Wood + 1×Gold + 1×Diamond
    ElderStick = 1,
    /// Броня характерника — 4×Leather + 2×Iron + 1×Gold
    Armour = 2,
    /// Бойовий браслет — 4×Iron + 2×Gold + 2×Diamond
    Brace = 3,
}

impl ItemType {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::KozakSable),
            1 => Some(Self::ElderStick),
            2 => Some(Self::Armour),
            3 => Some(Self::Brace),
            _ => None,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::KozakSable => "Kozak Sable",
            Self::ElderStick => "Elder Stick",
            Self::Armour => "Armour",
            Self::Brace => "Brace",
        }
    }

    pub fn symbol(&self) -> &'static str {
        match self {
            Self::KozakSable => "SABLE",
            Self::ElderStick => "STICK",
            Self::Armour => "ARMR",
            Self::Brace => "BRACE",
        }
    }

    pub fn uri(&self) -> &'static str {
        match self {
            Self::KozakSable => "https://arweave.net/kozak-sable",
            Self::ElderStick => "https://arweave.net/elder-stick",
            Self::Armour => "https://arweave.net/armour",
            Self::Brace => "https://arweave.net/brace",
        }
    }
}

#[program]
pub mod item_nft {
    use super::*;

    /// Initialize the item NFT configuration.
    pub fn initialize(ctx: Context<Initialize>) -> Result<()> {
        let config = &mut ctx.accounts.config;
        config.admin = ctx.accounts.admin.key();
        config.bump = ctx.bumps.config;
        config.item_authority_bump = ctx.bumps.item_authority;
        Ok(())
    }

    /// Set the crafting and marketplace program authority PDAs.
    pub fn set_authorities(
        ctx: Context<SetAuthorities>,
        crafting_authority: Pubkey,
        marketplace_authority: Pubkey,
    ) -> Result<()> {
        let config = &mut ctx.accounts.config;
        config.crafting_program_authority = crafting_authority;
        config.marketplace_program_authority = marketplace_authority;
        Ok(())
    }

    /// Mint a new NFT item to a player. Only callable by the crafting program via CPI.
    pub fn mint_item(ctx: Context<MintItem>, item_type: u8) -> Result<()> {
        require!(
            ctx.accounts.caller_authority.key()
                == ctx.accounts.config.crafting_program_authority,
            ItemNftError::Unauthorized
        );

        let it = ItemType::from_u8(item_type).ok_or(ItemNftError::InvalidItemType)?;

        let authority_bump = ctx.accounts.config.item_authority_bump;
        let authority_seeds: &[&[u8]] = &[b"item_authority", &[authority_bump]];
        let signer_seeds = &[authority_seeds];

        let token_metadata_program = ctx.accounts.token_metadata_program.to_account_info();

        // 1. Create the Metaplex metadata + master edition accounts.
        CreateV1CpiBuilder::new(&token_metadata_program)
            .metadata(&ctx.accounts.metadata.to_account_info())
            .master_edition(Some(&ctx.accounts.master_edition.to_account_info()))
            .mint(&ctx.accounts.mint.to_account_info(), true)
            .authority(&ctx.accounts.item_authority.to_account_info())
            .payer(&ctx.accounts.fee_payer.to_account_info())
            .update_authority(&ctx.accounts.item_authority.to_account_info(), true)
            .system_program(&ctx.accounts.system_program.to_account_info())
            .sysvar_instructions(&ctx.accounts.sysvar_instructions.to_account_info())
            .spl_token_program(Some(&ctx.accounts.token_program.to_account_info()))
            .name(it.name().to_string())
            .symbol(it.symbol().to_string())
            .uri(it.uri().to_string())
            .seller_fee_basis_points(0)
            .is_mutable(false)
            .token_standard(TokenStandard::NonFungible)
            .print_supply(PrintSupply::Zero)
            .invoke_signed(signer_seeds)?;

        // 2. Mint 1 token to the player's ATA.
        MintV1CpiBuilder::new(&token_metadata_program)
            .token(&ctx.accounts.player_ata.to_account_info())
            .token_owner(Some(&ctx.accounts.player.to_account_info()))
            .metadata(&ctx.accounts.metadata.to_account_info())
            .master_edition(Some(&ctx.accounts.master_edition.to_account_info()))
            .token_record(None)
            .mint(&ctx.accounts.mint.to_account_info())
            .authority(&ctx.accounts.item_authority.to_account_info())
            .delegate_record(None)
            .payer(&ctx.accounts.fee_payer.to_account_info())
            .system_program(&ctx.accounts.system_program.to_account_info())
            .sysvar_instructions(&ctx.accounts.sysvar_instructions.to_account_info())
            .spl_token_program(&ctx.accounts.token_program.to_account_info())
            .spl_ata_program(&ctx.accounts.associated_token_program.to_account_info())
            .amount(1)
            .invoke_signed(signer_seeds)?;

        // 3. Record item metadata in a PDA for later lookup.
        let item_meta = &mut ctx.accounts.item_metadata;
        item_meta.item_type = item_type;
        item_meta.owner = ctx.accounts.player.key();
        item_meta.mint = ctx.accounts.mint.key();
        item_meta.bump = ctx.bumps.item_metadata;

        Ok(())
    }

    /// Burn an NFT item. Only callable by the marketplace program via CPI.
    pub fn burn_item(ctx: Context<BurnItem>) -> Result<()> {
        require!(
            ctx.accounts.caller_authority.key()
                == ctx.accounts.config.marketplace_program_authority,
            ItemNftError::Unauthorized
        );

        let authority_bump = ctx.accounts.config.item_authority_bump;
        let authority_seeds: &[&[u8]] = &[b"item_authority", &[authority_bump]];
        let signer_seeds = &[authority_seeds];

        let token_metadata_program = ctx.accounts.token_metadata_program.to_account_info();

        BurnV1CpiBuilder::new(&token_metadata_program)
            .authority(&ctx.accounts.holder.to_account_info())
            .collection_metadata(None)
            .metadata(&ctx.accounts.metadata.to_account_info())
            .edition(Some(&ctx.accounts.master_edition.to_account_info()))
            .mint(&ctx.accounts.mint.to_account_info())
            .token(&ctx.accounts.holder_ata.to_account_info())
            .master_edition(None)
            .master_edition_mint(None)
            .master_edition_token(None)
            .edition_marker(None)
            .token_record(None)
            .system_program(&ctx.accounts.system_program.to_account_info())
            .sysvar_instructions(&ctx.accounts.sysvar_instructions.to_account_info())
            .spl_token_program(&ctx.accounts.token_program.to_account_info())
            .amount(1)
            .invoke_signed(signer_seeds)?;

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
        space = ItemConfig::SIZE,
        seeds = [b"item_config"],
        bump,
    )]
    pub config: Account<'info, ItemConfig>,

    #[account(
        seeds = [b"item_authority"],
        bump,
    )]
    /// CHECK: PDA signer — acts as update/mint authority for all item NFTs
    pub item_authority: UncheckedAccount<'info>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct SetAuthorities<'info> {
    pub admin: Signer<'info>,

    #[account(
        mut,
        seeds = [b"item_config"],
        bump = config.bump,
        has_one = admin,
    )]
    pub config: Account<'info, ItemConfig>,
}

#[derive(Accounts)]
pub struct MintItem<'info> {
    /// The crafting program's authority PDA.
    pub caller_authority: Signer<'info>,

    #[account(
        seeds = [b"item_config"],
        bump = config.bump,
    )]
    pub config: Box<Account<'info, ItemConfig>>,

    /// CHECK: PDA that is the mint/update authority for all items.
    #[account(
        seeds = [b"item_authority"],
        bump = config.item_authority_bump,
    )]
    pub item_authority: UncheckedAccount<'info>,

    /// Fresh mint keypair for this NFT — must be provided as signer by the crafting program.
    #[account(
        init,
        payer = fee_payer,
        mint::decimals = 0,
        mint::authority = item_authority,
        mint::freeze_authority = item_authority,
    )]
    pub mint: Box<Account<'info, Mint>>,

    /// CHECK: Metaplex metadata PDA — address validated by Metaplex program internally.
    #[account(mut)]
    pub metadata: UncheckedAccount<'info>,

    /// CHECK: Metaplex master edition PDA — address validated by Metaplex program internally.
    #[account(mut)]
    pub master_edition: UncheckedAccount<'info>,

    /// CHECK: Player receiving the NFT
    pub player: UncheckedAccount<'info>,

    #[account(
        init_if_needed,
        payer = fee_payer,
        associated_token::mint = mint,
        associated_token::authority = player,
    )]
    pub player_ata: Box<Account<'info, TokenAccount>>,

    /// Per-item metadata PDA storing type/owner/mint.
    #[account(
        init,
        payer = fee_payer,
        space = ItemMetadata::SIZE,
        seeds = [b"item_metadata", mint.key().as_ref()],
        bump,
    )]
    pub item_metadata: Box<Account<'info, ItemMetadata>>,

    #[account(mut)]
    pub fee_payer: Signer<'info>,

    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
    /// CHECK: Sysvar instructions account
    #[account(address = anchor_lang::solana_program::sysvar::instructions::id())]
    pub sysvar_instructions: UncheckedAccount<'info>,
    /// CHECK: Metaplex Token Metadata program
    #[account(address = mpl_token_metadata::ID)]
    pub token_metadata_program: UncheckedAccount<'info>,
}

#[derive(Accounts)]
pub struct BurnItem<'info> {
    /// The marketplace program's authority PDA.
    pub caller_authority: Signer<'info>,

    #[account(
        seeds = [b"item_config"],
        bump = config.bump,
    )]
    pub config: Box<Account<'info, ItemConfig>>,

    /// CHECK: PDA authority
    #[account(
        seeds = [b"item_authority"],
        bump = config.item_authority_bump,
    )]
    pub item_authority: UncheckedAccount<'info>,

    pub holder: Signer<'info>,

    #[account(mut)]
    pub mint: Box<Account<'info, Mint>>,

    /// CHECK: Metaplex metadata PDA — validated by Metaplex program internally.
    #[account(mut)]
    pub metadata: UncheckedAccount<'info>,

    /// CHECK: Metaplex master edition PDA — validated by Metaplex program internally.
    #[account(mut)]
    pub master_edition: UncheckedAccount<'info>,

    #[account(
        mut,
        associated_token::mint = mint,
        associated_token::authority = holder,
    )]
    pub holder_ata: Box<Account<'info, TokenAccount>>,

    /// Close the item metadata PDA on burn; rent goes back to holder.
    #[account(
        mut,
        seeds = [b"item_metadata", mint.key().as_ref()],
        bump = item_metadata.bump,
        close = holder,
    )]
    pub item_metadata: Box<Account<'info, ItemMetadata>>,

    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
    /// CHECK: Sysvar instructions
    #[account(address = anchor_lang::solana_program::sysvar::instructions::id())]
    pub sysvar_instructions: UncheckedAccount<'info>,
    /// CHECK: Metaplex Token Metadata program
    #[account(address = mpl_token_metadata::ID)]
    pub token_metadata_program: UncheckedAccount<'info>,
}

// ─── State ───────────────────────────────────────────────────────────────────

/// Global configuration for the item NFT program.
#[account]
pub struct ItemConfig {
    pub admin: Pubkey,
    /// PDA of the crafting program, allowed to mint items.
    pub crafting_program_authority: Pubkey,
    /// PDA of the marketplace program, allowed to burn items.
    pub marketplace_program_authority: Pubkey,
    pub bump: u8,
    pub item_authority_bump: u8,
}

impl ItemConfig {
    pub const SIZE: usize = 8 + 32 + 32 + 32 + 1 + 1;
}

/// Per-item on-chain metadata stored in a PDA (seeds: [b"item_metadata", mint]).
#[account]
pub struct ItemMetadata {
    /// ItemType as u8.
    pub item_type: u8,
    pub owner: Pubkey,
    pub mint: Pubkey,
    pub bump: u8,
}

impl ItemMetadata {
    pub const SIZE: usize = 8 + 1 + 32 + 32 + 1;
}

// ─── Errors ──────────────────────────────────────────────────────────────────

#[error_code]
pub enum ItemNftError {
    #[msg("Caller is not an authorized program authority")]
    Unauthorized,
    #[msg("Invalid item type — must be 0..3")]
    InvalidItemType,
}
