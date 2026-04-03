use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token_interface::{self, Mint, MintTo, TokenAccount, TokenInterface, Burn},
};

// TODO: Replace with actual program ID after `anchor build && anchor keys sync`
declare_id!("9ZjvRoFpEJ77XTX8sJWneBz1XLeYE9M5MHkDfer52L5i");

#[program]
pub mod magic_token {
    use super::*;

    /// Initialize the MagicToken configuration and SPL Token-2022 mint.
    /// Must be called once by the admin.
    pub fn initialize(ctx: Context<Initialize>) -> Result<()> {
        let config = &mut ctx.accounts.config;
        config.admin = ctx.accounts.admin.key();
        config.mint = ctx.accounts.mint.key();
        config.bump = ctx.bumps.config;
        config.mint_authority_bump = ctx.bumps.mint_authority;
        Ok(())
    }

    /// Set the marketplace program authority PDA that is allowed to mint/burn MagicTokens.
    /// marketplace_authority = PDA of marketplace program (seeds: [b"marketplace_authority"])
    pub fn set_marketplace_authority(
        ctx: Context<SetMarketplaceAuthority>,
        marketplace_authority: Pubkey,
    ) -> Result<()> {
        ctx.accounts.config.marketplace_authority = marketplace_authority;
        Ok(())
    }

    /// Mint MagicTokens to a recipient. Only callable by the marketplace program via CPI.
    pub fn mint_magic_tokens(ctx: Context<MintMagicTokens>, amount: u64) -> Result<()> {
        require!(
            ctx.accounts.caller_authority.key()
                == ctx.accounts.config.marketplace_authority,
            MagicTokenError::Unauthorized
        );
        require!(amount > 0, MagicTokenError::ZeroAmount);

        let seeds: &[&[u8]] = &[b"mint_authority", &[ctx.accounts.config.mint_authority_bump]];
        let signer_seeds = &[seeds];

        token_interface::mint_to(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                MintTo {
                    mint: ctx.accounts.mint.to_account_info(),
                    to: ctx.accounts.recipient_ata.to_account_info(),
                    authority: ctx.accounts.mint_authority.to_account_info(),
                },
                signer_seeds,
            ),
            amount,
        )?;

        Ok(())
    }

    /// Burn MagicTokens from a holder. Only callable by the marketplace program via CPI.
    pub fn burn_magic_tokens(ctx: Context<BurnMagicTokens>, amount: u64) -> Result<()> {
        require!(
            ctx.accounts.caller_authority.key()
                == ctx.accounts.config.marketplace_authority,
            MagicTokenError::Unauthorized
        );
        require!(amount > 0, MagicTokenError::ZeroAmount);

        token_interface::burn(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                Burn {
                    mint: ctx.accounts.mint.to_account_info(),
                    from: ctx.accounts.holder_ata.to_account_info(),
                    authority: ctx.accounts.holder.to_account_info(),
                },
            ),
            amount,
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
        space = MagicTokenConfig::SIZE,
        seeds = [b"magic_token_config"],
        bump,
    )]
    pub config: Account<'info, MagicTokenConfig>,

    /// The MagicToken SPL Token-2022 mint.
    #[account(
        init,
        payer = admin,
        mint::decimals = 0,
        mint::authority = mint_authority,
        mint::token_program = token_program,
    )]
    pub mint: Box<InterfaceAccount<'info, Mint>>,

    /// PDA that is the mint authority.
    #[account(
        seeds = [b"mint_authority"],
        bump,
    )]
    /// CHECK: PDA signer
    pub mint_authority: UncheckedAccount<'info>,

    pub token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
pub struct SetMarketplaceAuthority<'info> {
    pub admin: Signer<'info>,

    #[account(
        mut,
        seeds = [b"magic_token_config"],
        bump = config.bump,
        has_one = admin,
    )]
    pub config: Account<'info, MagicTokenConfig>,
}

#[derive(Accounts)]
pub struct MintMagicTokens<'info> {
    /// The marketplace program's authority PDA.
    pub caller_authority: Signer<'info>,

    #[account(
        seeds = [b"magic_token_config"],
        bump = config.bump,
        has_one = mint,
    )]
    pub config: Account<'info, MagicTokenConfig>,

    #[account(mut)]
    pub mint: Box<InterfaceAccount<'info, Mint>>,

    #[account(
        seeds = [b"mint_authority"],
        bump = config.mint_authority_bump,
    )]
    /// CHECK: PDA signer
    pub mint_authority: UncheckedAccount<'info>,

    /// CHECK: Recipient wallet
    pub recipient: UncheckedAccount<'info>,

    #[account(
        init_if_needed,
        payer = fee_payer,
        associated_token::mint = mint,
        associated_token::authority = recipient,
        associated_token::token_program = token_program,
    )]
    pub recipient_ata: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(mut)]
    pub fee_payer: Signer<'info>,

    pub token_program: Interface<'info, TokenInterface>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct BurnMagicTokens<'info> {
    /// The marketplace program's authority PDA.
    pub caller_authority: Signer<'info>,

    #[account(
        seeds = [b"magic_token_config"],
        bump = config.bump,
        has_one = mint,
    )]
    pub config: Account<'info, MagicTokenConfig>,

    #[account(mut)]
    pub mint: Box<InterfaceAccount<'info, Mint>>,

    /// Token holder — must sign (passed via CPI from marketplace).
    pub holder: Signer<'info>,

    #[account(
        mut,
        associated_token::mint = mint,
        associated_token::authority = holder,
        associated_token::token_program = token_program,
    )]
    pub holder_ata: Box<InterfaceAccount<'info, TokenAccount>>,

    pub token_program: Interface<'info, TokenInterface>,
}

// ─── State ───────────────────────────────────────────────────────────────────

/// Configuration for the MagicToken program.
#[account]
pub struct MagicTokenConfig {
    pub admin: Pubkey,
    pub mint: Pubkey,
    /// PDA of the marketplace program, the sole authority for mint/burn.
    pub marketplace_authority: Pubkey,
    pub bump: u8,
    pub mint_authority_bump: u8,
}

impl MagicTokenConfig {
    pub const SIZE: usize = 8 + 32 + 32 + 32 + 1 + 1;
}

// ─── Errors ──────────────────────────────────────────────────────────────────

#[error_code]
pub enum MagicTokenError {
    #[msg("Caller is not the authorized marketplace program authority")]
    Unauthorized,
    #[msg("Amount must be greater than zero")]
    ZeroAmount,
}
