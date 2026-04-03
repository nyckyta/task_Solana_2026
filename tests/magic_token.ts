import * as anchor from "@coral-xyz/anchor";
import { Program, BN } from "@coral-xyz/anchor";
import { MagicToken } from "../target/types/magic_token";
import {
  TOKEN_2022_PROGRAM_ID,
  getAssociatedTokenAddressSync,
  getAccount,
  createAssociatedTokenAccountInstruction,
} from "@solana/spl-token";
import { Keypair, PublicKey, SystemProgram } from "@solana/web3.js";
import { assert } from "chai";

describe("magic_token", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);
  const program = anchor.workspace.MagicToken as Program<MagicToken>;
  const admin = provider.wallet as anchor.Wallet;

  const mintKeypair = Keypair.generate();
  const [config] = PublicKey.findProgramAddressSync(
    [Buffer.from("magic_token_config")],
    program.programId
  );
  const [mintAuthority] = PublicKey.findProgramAddressSync(
    [Buffer.from("mint_authority")],
    program.programId
  );

  const mockMarketplaceAuthority = Keypair.generate();

  it("initializes config and mint", async () => {
    await program.methods
      .initialize()
      .accounts({
        admin: admin.publicKey,
        config,
        mint: mintKeypair.publicKey,
        mintAuthority,
        tokenProgram: TOKEN_2022_PROGRAM_ID,
        systemProgram: SystemProgram.programId,
        rent: anchor.web3.SYSVAR_RENT_PUBKEY,
      })
      .signers([mintKeypair])
      .rpc();

    const cfg = await program.account.magicTokenConfig.fetch(config);
    assert.ok(cfg.admin.equals(admin.publicKey));
    assert.ok(cfg.mint.equals(mintKeypair.publicKey));
  });

  it("sets marketplace authority", async () => {
    await program.methods
      .setMarketplaceAuthority(mockMarketplaceAuthority.publicKey)
      .accounts({ admin: admin.publicKey, config })
      .rpc();

    const cfg = await program.account.magicTokenConfig.fetch(config);
    assert.ok(cfg.marketplaceAuthority.equals(mockMarketplaceAuthority.publicKey));
  });

  it("mints tokens when called by marketplace authority", async () => {
    const recipient = Keypair.generate();
    await provider.connection.confirmTransaction(
      await provider.connection.requestAirdrop(recipient.publicKey, 2e9)
    );

    const recipientAta = getAssociatedTokenAddressSync(
      mintKeypair.publicKey,
      recipient.publicKey,
      false,
      TOKEN_2022_PROGRAM_ID
    );

    await program.methods
      .mintMagicTokens(new BN(100))
      .accounts({
        callerAuthority: mockMarketplaceAuthority.publicKey,
        config,
        mint: mintKeypair.publicKey,
        mintAuthority,
        recipient: recipient.publicKey,
        recipientAta,
        feePayer: admin.publicKey,
        tokenProgram: TOKEN_2022_PROGRAM_ID,
        associatedTokenProgram: anchor.utils.token.ASSOCIATED_PROGRAM_ID,
        systemProgram: SystemProgram.programId,
      })
      .signers([mockMarketplaceAuthority])
      .rpc();

    const ataInfo = await getAccount(
      provider.connection,
      recipientAta,
      undefined,
      TOKEN_2022_PROGRAM_ID
    );
    assert.equal(ataInfo.amount.toString(), "100");
  });

  it("rejects mint from unauthorized caller", async () => {
    const badActor = Keypair.generate();
    const recipientAta = getAssociatedTokenAddressSync(
      mintKeypair.publicKey,
      admin.publicKey,
      false,
      TOKEN_2022_PROGRAM_ID
    );
    try {
      await program.methods
        .mintMagicTokens(new BN(1))
        .accounts({
          callerAuthority: badActor.publicKey,
          config,
          mint: mintKeypair.publicKey,
          mintAuthority,
          recipient: admin.publicKey,
          recipientAta,
          feePayer: admin.publicKey,
          tokenProgram: TOKEN_2022_PROGRAM_ID,
          associatedTokenProgram: anchor.utils.token.ASSOCIATED_PROGRAM_ID,
          systemProgram: SystemProgram.programId,
        })
        .signers([badActor])
        .rpc();
      assert.fail("should have thrown Unauthorized");
    } catch (e: any) {
      assert.include(e.message, "Unauthorized");
    }
  });

  it("burns tokens when called by marketplace authority", async () => {
    // First mint some tokens to admin
    const holderAta = getAssociatedTokenAddressSync(
      mintKeypair.publicKey,
      admin.publicKey,
      false,
      TOKEN_2022_PROGRAM_ID
    );
    // Ensure ATA exists
    try {
      const createAtaIx = createAssociatedTokenAccountInstruction(
        admin.publicKey,
        holderAta,
        admin.publicKey,
        mintKeypair.publicKey,
        TOKEN_2022_PROGRAM_ID
      );
      await provider.sendAndConfirm(
        new anchor.web3.Transaction().add(createAtaIx)
      );
    } catch (_) {}

    await program.methods
      .mintMagicTokens(new BN(50))
      .accounts({
        callerAuthority: mockMarketplaceAuthority.publicKey,
        config,
        mint: mintKeypair.publicKey,
        mintAuthority,
        recipient: admin.publicKey,
        recipientAta: holderAta,
        feePayer: admin.publicKey,
        tokenProgram: TOKEN_2022_PROGRAM_ID,
        associatedTokenProgram: anchor.utils.token.ASSOCIATED_PROGRAM_ID,
        systemProgram: SystemProgram.programId,
      })
      .signers([mockMarketplaceAuthority])
      .rpc();

    await program.methods
      .burnMagicTokens(new BN(20))
      .accounts({
        callerAuthority: mockMarketplaceAuthority.publicKey,
        config,
        mint: mintKeypair.publicKey,
        holder: admin.publicKey,
        holderAta,
        tokenProgram: TOKEN_2022_PROGRAM_ID,
      })
      .signers([mockMarketplaceAuthority, admin.payer])
      .rpc();

    const ataInfo = await getAccount(
      provider.connection,
      holderAta,
      undefined,
      TOKEN_2022_PROGRAM_ID
    );
    assert.equal(ataInfo.amount.toString(), "30");
  });

  it("rejects burn from unauthorized caller", async () => {
    const badActor = Keypair.generate();
    const holderAta = getAssociatedTokenAddressSync(
      mintKeypair.publicKey,
      admin.publicKey,
      false,
      TOKEN_2022_PROGRAM_ID
    );
    try {
      await program.methods
        .burnMagicTokens(new BN(1))
        .accounts({
          callerAuthority: badActor.publicKey,
          config,
          mint: mintKeypair.publicKey,
          holder: admin.publicKey,
          holderAta,
          tokenProgram: TOKEN_2022_PROGRAM_ID,
        })
        .signers([badActor, admin.payer])
        .rpc();
      assert.fail("should have thrown Unauthorized");
    } catch (e: any) {
      assert.include(e.message, "Unauthorized");
    }
  });

  it("rejects zero amount mint", async () => {
    const recipientAta = getAssociatedTokenAddressSync(
      mintKeypair.publicKey,
      admin.publicKey,
      false,
      TOKEN_2022_PROGRAM_ID
    );
    try {
      await program.methods
        .mintMagicTokens(new BN(0))
        .accounts({
          callerAuthority: mockMarketplaceAuthority.publicKey,
          config,
          mint: mintKeypair.publicKey,
          mintAuthority,
          recipient: admin.publicKey,
          recipientAta,
          feePayer: admin.publicKey,
          tokenProgram: TOKEN_2022_PROGRAM_ID,
          associatedTokenProgram: anchor.utils.token.ASSOCIATED_PROGRAM_ID,
          systemProgram: SystemProgram.programId,
        })
        .signers([mockMarketplaceAuthority])
        .rpc();
      assert.fail("should have thrown ZeroAmount");
    } catch (e: any) {
      assert.include(e.message, "ZeroAmount");
    }
  });
});
