use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{self, Token, TokenAccount, Transfer},
    token_interface::{self, TokenInterface, InterfaceAccount},
};
use item_nft::{
    cpi::{accounts::BurnItem, burn_item},
    program::ItemNft,
    ItemConfig, ItemMetadata,
};
use magic_token::{
    cpi::{
        accounts::{BurnMagicTokens, MintMagicTokens},
        burn_magic_tokens, mint_magic_tokens,
    },
    program::MagicToken,
    MagicTokenConfig,
};

// TODO: Replace with actual program ID after `anchor build && anchor keys sync`
declare_id!("A5tgKAc9BTqyemxXSsieX3eijcAWEBcoLALSVvuyyQWX");

#[program]
pub mod marketplace {
    use super::*;

    /// Initialize the marketplace configuration PDA.
    pub fn initialize(ctx: Context<Initialize>) -> Result<()> {
        let config = &mut ctx.accounts.config;
        config.admin = ctx.accounts.admin.key();
        config.bump = ctx.bumps.config;
        config.marketplace_authority_bump = ctx.bumps.marketplace_authority;
        Ok(())
    }

    /// List an NFT item for sale.
    ///
    /// The seller must own the item (ATA balance = 1). Price must be non-zero.
    pub fn list_item(ctx: Context<ListItem>, price: u64) -> Result<()> {
        require!(price > 0, MarketplaceError::ZeroPrice);

        // Verify the seller actually holds the NFT.
        require!(
            ctx.accounts.seller_ata.amount == 1,
            MarketplaceError::NotItemOwner
        );

        let listing = &mut ctx.accounts.listing;
        listing.seller = ctx.accounts.seller.key();
        listing.item_mint = ctx.accounts.item_mint.key();
        listing.price = price;
        listing.bump = ctx.bumps.listing;

        Ok(())
    }

    /// Cancel a listing and remove the Listing PDA.
    pub fn cancel_listing(ctx: Context<CancelListing>) -> Result<()> {
        // has_one = seller enforced in the Accounts struct.
        Ok(())
    }

    /// Buy a listed item.
    ///
    /// Flow:
    /// 1. Burn MagicTokens from the buyer (price amount).
    /// 2. Transfer the NFT from seller to buyer.
    /// 3. Mint MagicTokens to the seller (price amount).
    /// 4. Close the Listing PDA (rent reclaimed by seller).
    pub fn buy_item(ctx: &Context<BuyItem>) -> Result<()> {
        let price = ctx.accounts.listing.price;
        let authority_bump = ctx.accounts.config.marketplace_authority_bump;
        let authority_seeds: &[&[u8]] = &[b"marketplace_authority", &[authority_bump]];
        let signer_seeds = &[authority_seeds];

        // 1. Burn MagicTokens from the buyer.
        burn_magic_tokens(
            CpiContext::new_with_signer(
                ctx.accounts.magic_token_program.to_account_info(),
                BurnMagicTokens {
                    caller_authority: ctx.accounts.marketplace_authority.to_account_info(),
                    config: ctx.accounts.magic_token_config.to_account_info(),
                    mint: ctx.accounts.magic_token_mint.to_account_info(),
                    holder: ctx.accounts.buyer.to_account_info(),
                    holder_ata: ctx.accounts.buyer_magic_ata.to_account_info(),
                    token_program: ctx.accounts.magic_token_program_interface.to_account_info(),
                },
                signer_seeds,
            ),
            price,
        )?;

        // 2. Transfer the NFT from seller to buyer via SPL token transfer.
        //    The listing PDA holds no authority; seller must sign via the marketplace CPI.
        //    Since the seller signed the original transaction (buy_item requires seller_ata
        //    to be mutable and seller to sign), we transfer directly.
        token::transfer(
            CpiContext::new(
                ctx.accounts.spl_token_program.to_account_info(),
                Transfer {
                    from: ctx.accounts.seller_item_ata.to_account_info(),
                    to: ctx.accounts.buyer_item_ata.to_account_info(),
                    authority: ctx.accounts.seller.to_account_info(),
                },
            ),
            1,
        )?;

        // 3. Mint MagicTokens to the seller.
        mint_magic_tokens(
            CpiContext::new_with_signer(
                ctx.accounts.magic_token_program.to_account_info(),
                MintMagicTokens {
                    caller_authority: ctx.accounts.marketplace_authority.to_account_info(),
                    config: ctx.accounts.magic_token_config.to_account_info(),
                    mint: ctx.accounts.magic_token_mint.to_account_info(),
                    mint_authority: ctx.accounts.magic_token_mint_authority.to_account_info(),
                    recipient: ctx.accounts.seller.to_account_info(),
                    recipient_ata: ctx.accounts.seller_magic_ata.to_account_info(),
                    fee_payer: ctx.accounts.buyer.to_account_info(),
                    token_program: ctx.accounts.magic_token_program_interface.to_account_info(),
                    associated_token_program: ctx.accounts.associated_token_program.to_account_info(),
                    system_program: ctx.accounts.system_program.to_account_info(),
                },
                signer_seeds,
            ),
            price,
        )?;

        Ok(())
    }

    /// Sell (list + immediately burn NFT) to receive MagicTokens.
    ///
    /// This is the "direct sell" flow: the NFT is burned and the seller gets MagicTokens.
    /// No buyer is involved — this mirrors the Whitechain reference where the buyer
    /// triggers a burn. Here we implement it as a seller-initiated action that burns the
    /// NFT and mints tokens to the seller in one step.
    pub fn sell_item(ctx: Context<SellItem>, price: u64) -> Result<()> {
        require!(price > 0, MarketplaceError::ZeroPrice);
        require!(ctx.accounts.seller_item_ata.amount == 1, MarketplaceError::NotItemOwner);

        let authority_bump = ctx.accounts.config.marketplace_authority_bump;
        let authority_seeds: &[&[u8]] = &[b"marketplace_authority", &[authority_bump]];
        let signer_seeds = &[authority_seeds];

        // Burn the NFT item via item_nft CPI.
        burn_item(
            CpiContext::new_with_signer(
                ctx.accounts.item_nft_program.to_account_info(),
                BurnItem {
                    caller_authority: ctx.accounts.marketplace_authority.to_account_info(),
                    config: ctx.accounts.item_config.to_account_info(),
                    item_authority: ctx.accounts.item_authority.to_account_info(),
                    holder: ctx.accounts.seller.to_account_info(),
                    mint: ctx.accounts.item_mint.to_account_info(),
                    metadata: ctx.accounts.item_metadata_account.to_account_info(),
                    master_edition: ctx.accounts.master_edition.to_account_info(),
                    holder_ata: ctx.accounts.seller_item_ata.to_account_info(),
                    item_metadata: ctx.accounts.item_metadata.to_account_info(),
                    token_program: ctx.accounts.spl_token_program.to_account_info(),
                    system_program: ctx.accounts.system_program.to_account_info(),
                    sysvar_instructions: ctx.accounts.sysvar_instructions.to_account_info(),
                    token_metadata_program: ctx.accounts.token_metadata_program.to_account_info(),
                },
                signer_seeds,
            ),
        )?;

        // Mint MagicTokens to the seller.
        mint_magic_tokens(
            CpiContext::new_with_signer(
                ctx.accounts.magic_token_program.to_account_info(),
                MintMagicTokens {
                    caller_authority: ctx.accounts.marketplace_authority.to_account_info(),
                    config: ctx.accounts.magic_token_config.to_account_info(),
                    mint: ctx.accounts.magic_token_mint.to_account_info(),
                    mint_authority: ctx.accounts.magic_token_mint_authority.to_account_info(),
                    recipient: ctx.accounts.seller.to_account_info(),
                    recipient_ata: ctx.accounts.seller_magic_ata.to_account_info(),
                    fee_payer: ctx.accounts.seller.to_account_info(),
                    token_program: ctx.accounts.magic_token_program_interface.to_account_info(),
                    associated_token_program: ctx.accounts.associated_token_program.to_account_info(),
                    system_program: ctx.accounts.system_program.to_account_info(),
                },
                signer_seeds,
            ),
            price,
        )?;

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
        space = MarketplaceConfig::SIZE,
        seeds = [b"marketplace_config"],
        bump,
    )]
    pub config: Account<'info, MarketplaceConfig>,

    #[account(
        seeds = [b"marketplace_authority"],
        bump,
    )]
    /// CHECK: PDA signer
    pub marketplace_authority: UncheckedAccount<'info>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct ListItem<'info> {
    #[account(mut)]
    pub seller: Signer<'info>,

    #[account(
        seeds = [b"marketplace_config"],
        bump = config.bump,
    )]
    pub config: Account<'info, MarketplaceConfig>,

    /// CHECK: The NFT mint being listed
    pub item_mint: UncheckedAccount<'info>,

    #[account(
        associated_token::mint = item_mint,
        associated_token::authority = seller,
    )]
    pub seller_ata: Box<Account<'info, TokenAccount>>,

    #[account(
        init,
        payer = seller,
        space = Listing::SIZE,
        seeds = [b"listing", item_mint.key().as_ref()],
        bump,
    )]
    pub listing: Account<'info, Listing>,

    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct CancelListing<'info> {
    #[account(mut)]
    pub seller: Signer<'info>,

    #[account(
        mut,
        seeds = [b"listing", listing.item_mint.as_ref()],
        bump = listing.bump,
        has_one = seller @ MarketplaceError::NotSeller,
        close = seller,
    )]
    pub listing: Account<'info, Listing>,
}

#[derive(Accounts)]
pub struct BuyItem<'info> {
    #[account(mut)]
    pub buyer: Signer<'info>,

    /// The seller must sign so we can use their ATA as transfer authority.
    #[account(mut)]
    pub seller: Signer<'info>,

    #[account(
        seeds = [b"marketplace_config"],
        bump = config.bump,
    )]
    pub config: Account<'info, MarketplaceConfig>,

    /// CHECK: marketplace authority PDA
    #[account(
        seeds = [b"marketplace_authority"],
        bump = config.marketplace_authority_bump,
    )]
    pub marketplace_authority: UncheckedAccount<'info>,

    #[account(
        mut,
        seeds = [b"listing", listing.item_mint.as_ref()],
        bump = listing.bump,
        has_one = seller @ MarketplaceError::NotSeller,
        close = seller,
    )]
    pub listing: Account<'info, Listing>,

    // ── NFT transfer ──────────────────────────────────────────────────────
    /// CHECK: The NFT mint
    pub item_mint: UncheckedAccount<'info>,

    #[account(
        mut,
        associated_token::mint = item_mint,
        associated_token::authority = seller,
    )]
    pub seller_item_ata: Box<Account<'info, TokenAccount>>,

    #[account(
        init_if_needed,
        payer = buyer,
        associated_token::mint = item_mint,
        associated_token::authority = buyer,
    )]
    pub buyer_item_ata: Box<Account<'info, TokenAccount>>,

    // ── MagicToken burn from buyer ────────────────────────────────────────
    #[account(
        seeds = [b"magic_token_config"],
        seeds::program = magic_token_program.key(),
        bump = magic_token_config.bump,
    )]
    pub magic_token_config: Account<'info, MagicTokenConfig>,

    #[account(mut, address = magic_token_config.mint)]
    /// CHECK: magic token mint
    pub magic_token_mint: UncheckedAccount<'info>,

    /// CHECK: mint_authority PDA in magic_token program
    #[account(
        seeds = [b"mint_authority"],
        seeds::program = magic_token_program.key(),
        bump = magic_token_config.mint_authority_bump,
    )]
    pub magic_token_mint_authority: UncheckedAccount<'info>,

    #[account(
        mut,
        associated_token::mint = magic_token_mint,
        associated_token::authority = buyer,
        associated_token::token_program = magic_token_program_interface,
    )]
    pub buyer_magic_ata: Box<InterfaceAccount<'info, anchor_spl::token_interface::TokenAccount>>,

    #[account(
        init_if_needed,
        payer = buyer,
        associated_token::mint = magic_token_mint,
        associated_token::authority = seller,
        associated_token::token_program = magic_token_program_interface,
    )]
    pub seller_magic_ata: Box<InterfaceAccount<'info, anchor_spl::token_interface::TokenAccount>>,

    // ── Programs ──────────────────────────────────────────────────────────
    pub magic_token_program: Program<'info, MagicToken>,
    pub magic_token_program_interface: Interface<'info, TokenInterface>,
    pub item_nft_program: Program<'info, ItemNft>,
    pub spl_token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct SellItem<'info> {
    #[account(mut)]
    pub seller: Signer<'info>,

    #[account(
        seeds = [b"marketplace_config"],
        bump = config.bump,
    )]
    pub config: Account<'info, MarketplaceConfig>,

    /// CHECK: marketplace authority PDA
    #[account(
        seeds = [b"marketplace_authority"],
        bump = config.marketplace_authority_bump,
    )]
    pub marketplace_authority: UncheckedAccount<'info>,

    // ── NFT burn ─────────────────────────────────────────────────────────
    #[account(mut)]
    /// CHECK: The NFT mint being sold/burned
    pub item_mint: UncheckedAccount<'info>,

    #[account(
        mut,
        associated_token::mint = item_mint,
        associated_token::authority = seller,
    )]
    pub seller_item_ata: Box<Account<'info, TokenAccount>>,

    /// CHECK: Metaplex metadata PDA
    #[account(mut)]
    pub item_metadata_account: UncheckedAccount<'info>,

    /// CHECK: Metaplex master edition PDA
    #[account(mut)]
    pub master_edition: UncheckedAccount<'info>,

    /// Our on-chain ItemMetadata PDA (closed on burn; rent goes to seller).
    #[account(
        mut,
        seeds = [b"item_metadata", item_mint.key().as_ref()],
        seeds::program = item_nft_program.key(),
        bump = item_metadata.bump,
    )]
    pub item_metadata: Account<'info, ItemMetadata>,

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

    // ── MagicToken mint to seller ─────────────────────────────────────────
    #[account(
        seeds = [b"magic_token_config"],
        seeds::program = magic_token_program.key(),
        bump = magic_token_config.bump,
    )]
    pub magic_token_config: Account<'info, MagicTokenConfig>,

    #[account(mut, address = magic_token_config.mint)]
    /// CHECK: magic token mint
    pub magic_token_mint: UncheckedAccount<'info>,

    /// CHECK: mint_authority PDA in magic_token program
    #[account(
        seeds = [b"mint_authority"],
        seeds::program = magic_token_program.key(),
        bump = magic_token_config.mint_authority_bump,
    )]
    pub magic_token_mint_authority: UncheckedAccount<'info>,

    #[account(
        init_if_needed,
        payer = seller,
        associated_token::mint = magic_token_mint,
        associated_token::authority = seller,
        associated_token::token_program = magic_token_program_interface,
    )]
    pub seller_magic_ata: Box<InterfaceAccount<'info, anchor_spl::token_interface::TokenAccount>>,

    // ── Programs ──────────────────────────────────────────────────────────
    pub item_nft_program: Program<'info, ItemNft>,
    pub magic_token_program: Program<'info, MagicToken>,
    pub magic_token_program_interface: Interface<'info, TokenInterface>,
    pub spl_token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
    /// CHECK: Sysvar instructions
    #[account(address = anchor_lang::solana_program::sysvar::instructions::id())]
    pub sysvar_instructions: UncheckedAccount<'info>,
    /// CHECK: Metaplex Token Metadata program
    #[account(address = mpl_token_metadata::ID)]
    pub token_metadata_program: UncheckedAccount<'info>,
}

// ─── State ───────────────────────────────────────────────────────────────────

#[account]
pub struct MarketplaceConfig {
    pub admin: Pubkey,
    pub bump: u8,
    pub marketplace_authority_bump: u8,
}

impl MarketplaceConfig {
    pub const SIZE: usize = 8 + 32 + 1 + 1;
}

/// Active listing for an NFT item.
#[account]
pub struct Listing {
    pub seller: Pubkey,
    pub item_mint: Pubkey,
    /// Price in MagicTokens.
    pub price: u64,
    pub bump: u8,
}

impl Listing {
    pub const SIZE: usize = 8 + 32 + 32 + 8 + 1;
}

// ─── Errors ──────────────────────────────────────────────────────────────────

#[error_code]
pub enum MarketplaceError {
    #[msg("Price must be greater than zero")]
    ZeroPrice,
    #[msg("Signer does not hold this NFT item")]
    NotItemOwner,
    #[msg("Signer is not the seller for this listing")]
    NotSeller,
}
