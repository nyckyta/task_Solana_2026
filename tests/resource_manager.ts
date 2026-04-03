import * as anchor from "@coral-xyz/anchor";
import { Program, BN } from "@coral-xyz/anchor";
import { ResourceManager } from "../target/types/resource_manager";
import {
  TOKEN_2022_PROGRAM_ID,
  createAssociatedTokenAccountInstruction,
  getAssociatedTokenAddressSync,
  getAccount,
} from "@solana/spl-token";
import { Keypair, PublicKey, SystemProgram } from "@solana/web3.js";
import { assert } from "chai";

describe("resource_manager", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);
  const program = anchor.workspace.ResourceManager as Program<ResourceManager>;
  const admin = provider.wallet as anchor.Wallet;

  // ── PDAs ────────────────────────────────────────────────────────────────
  const [gameConfig] = PublicKey.findProgramAddressSync(
    [Buffer.from("game_config")],
    program.programId
  );
  const [resourceAuthority] = PublicKey.findProgramAddressSync(
    [Buffer.from("resource_authority")],
    program.programId
  );

  // We'll use a mock "search authority" keypair for testing
  const mockSearchAuthority = Keypair.generate();
  const mockCraftingAuthority = Keypair.generate();

  const mintKeypairs: Keypair[] = Array.from({ length: 6 }, () => Keypair.generate());

  it("initializes game config", async () => {
    await program.methods
      .initialize()
      .accounts({
        admin: admin.publicKey,
        gameConfig,
        systemProgram: SystemProgram.programId,
      })
      .rpc();

    const config = await program.account.gameConfig.fetch(gameConfig);
    assert.ok(config.admin.equals(admin.publicKey), "admin mismatch");
  });

  it("initializes all 6 resource mints", async () => {
    for (let i = 0; i < 6; i++) {
      await program.methods
        .initResourceMint(i, `https://arweave.net/resource-${i}`)
        .accounts({
          admin: admin.publicKey,
          gameConfig,
          mint: mintKeypairs[i].publicKey,
          resourceAuthority,
          tokenProgram: TOKEN_2022_PROGRAM_ID,
          systemProgram: SystemProgram.programId,
          rent: anchor.web3.SYSVAR_RENT_PUBKEY,
        })
        .signers([mintKeypairs[i]])
        .rpc();
    }

    const config = await program.account.gameConfig.fetch(gameConfig);
    for (let i = 0; i < 6; i++) {
      assert.ok(
        config.resourceMints[i].equals(mintKeypairs[i].publicKey),
        `resource mint ${i} not stored`
      );
    }
  });

  it("fails to initialize the same mint twice", async () => {
    try {
      await program.methods
        .initResourceMint(0, "https://arweave.net/resource-0")
        .accounts({
          admin: admin.publicKey,
          gameConfig,
          mint: Keypair.generate().publicKey,
          resourceAuthority,
          tokenProgram: TOKEN_2022_PROGRAM_ID,
          systemProgram: SystemProgram.programId,
          rent: anchor.web3.SYSVAR_RENT_PUBKEY,
        })
        .rpc();
      assert.fail("should have thrown");
    } catch (e: any) {
      assert.include(e.message, "MintAlreadyInitialized");
    }
  });

  it("sets search and crafting authorities", async () => {
    await program.methods
      .setAuthorities(
        mockSearchAuthority.publicKey,
        mockCraftingAuthority.publicKey
      )
      .accounts({ admin: admin.publicKey, gameConfig })
      .rpc();

    const config = await program.account.gameConfig.fetch(gameConfig);
    assert.ok(
      config.searchProgramAuthority.equals(mockSearchAuthority.publicKey)
    );
    assert.ok(
      config.craftingProgramAuthority.equals(mockCraftingAuthority.publicKey)
    );
  });

  it("mints resources when called by search authority", async () => {
    const player = Keypair.generate();
    // Airdrop SOL to player for ATA rent
    await provider.connection.confirmTransaction(
      await provider.connection.requestAirdrop(player.publicKey, 2e9)
    );

    const mint = mintKeypairs[0].publicKey;
    const playerAta = getAssociatedTokenAddressSync(
      mint,
      player.publicKey,
      false,
      TOKEN_2022_PROGRAM_ID
    );

    // Create the ATA first
    const createAtaIx = createAssociatedTokenAccountInstruction(
      player.publicKey,
      playerAta,
      player.publicKey,
      mint,
      TOKEN_2022_PROGRAM_ID
    );
    const tx = new anchor.web3.Transaction().add(createAtaIx);
    await provider.sendAndConfirm(tx, [player]);

    await program.methods
      .mintResources([0], [new BN(3)])
      .accounts({
        callerAuthority: mockSearchAuthority.publicKey,
        player: player.publicKey,
        gameConfig,
        resourceAuthority,
        tokenProgram: TOKEN_2022_PROGRAM_ID,
        associatedTokenProgram: anchor.utils.token.ASSOCIATED_PROGRAM_ID,
        systemProgram: SystemProgram.programId,
      })
      .remainingAccounts([
        { pubkey: mint, isWritable: true, isSigner: false },
        { pubkey: playerAta, isWritable: true, isSigner: false },
      ])
      .signers([mockSearchAuthority])
      .rpc();

    const ataInfo = await getAccount(
      provider.connection,
      playerAta,
      undefined,
      TOKEN_2022_PROGRAM_ID
    );
    assert.equal(ataInfo.amount.toString(), "3");
  });

  it("rejects mint_resources from unauthorized caller", async () => {
    const badActor = Keypair.generate();
    try {
      await program.methods
        .mintResources([0], [new BN(1)])
        .accounts({
          callerAuthority: badActor.publicKey,
          player: admin.publicKey,
          gameConfig,
          resourceAuthority,
          tokenProgram: TOKEN_2022_PROGRAM_ID,
          associatedTokenProgram: anchor.utils.token.ASSOCIATED_PROGRAM_ID,
          systemProgram: SystemProgram.programId,
        })
        .remainingAccounts([
          { pubkey: mintKeypairs[0].publicKey, isWritable: true, isSigner: false },
          {
            pubkey: getAssociatedTokenAddressSync(
              mintKeypairs[0].publicKey,
              admin.publicKey,
              false,
              TOKEN_2022_PROGRAM_ID
            ),
            isWritable: true,
            isSigner: false,
          },
        ])
        .signers([badActor])
        .rpc();
      assert.fail("should have thrown Unauthorized");
    } catch (e: any) {
      assert.include(e.message, "Unauthorized");
    }
  });

  it("burns resources when called by crafting authority", async () => {
    // Mint some tokens first (reuse search authority for this test setup)
    const player = Keypair.generate();
    await provider.connection.confirmTransaction(
      await provider.connection.requestAirdrop(player.publicKey, 2e9)
    );
    const mint = mintKeypairs[1].publicKey;
    const playerAta = getAssociatedTokenAddressSync(
      mint,
      player.publicKey,
      false,
      TOKEN_2022_PROGRAM_ID
    );

    const createAtaIx = createAssociatedTokenAccountInstruction(
      player.publicKey,
      playerAta,
      player.publicKey,
      mint,
      TOKEN_2022_PROGRAM_ID
    );
    await provider.sendAndConfirm(new anchor.web3.Transaction().add(createAtaIx), [player]);

    // Mint 5 tokens to player (using the mock search authority)
    await program.methods
      .mintResources([1], [new BN(5)])
      .accounts({
        callerAuthority: mockSearchAuthority.publicKey,
        player: player.publicKey,
        gameConfig,
        resourceAuthority,
        tokenProgram: TOKEN_2022_PROGRAM_ID,
        associatedTokenProgram: anchor.utils.token.ASSOCIATED_PROGRAM_ID,
        systemProgram: SystemProgram.programId,
      })
      .remainingAccounts([
        { pubkey: mint, isWritable: true, isSigner: false },
        { pubkey: playerAta, isWritable: true, isSigner: false },
      ])
      .signers([mockSearchAuthority])
      .rpc();

    // Now burn 3 of them
    await program.methods
      .burnResources([1], [new BN(3)])
      .accounts({
        callerAuthority: mockCraftingAuthority.publicKey,
        player: player.publicKey,
        gameConfig,
        tokenProgram: TOKEN_2022_PROGRAM_ID,
      })
      .remainingAccounts([
        { pubkey: mint, isWritable: true, isSigner: false },
        { pubkey: playerAta, isWritable: true, isSigner: false },
      ])
      .signers([mockCraftingAuthority, player])
      .rpc();

    const ataInfo = await getAccount(
      provider.connection,
      playerAta,
      undefined,
      TOKEN_2022_PROGRAM_ID
    );
    assert.equal(ataInfo.amount.toString(), "2");
  });

  it("rejects burn_resources from unauthorized caller", async () => {
    const badActor = Keypair.generate();
    try {
      await program.methods
        .burnResources([0], [new BN(1)])
        .accounts({
          callerAuthority: badActor.publicKey,
          player: admin.publicKey,
          gameConfig,
          tokenProgram: TOKEN_2022_PROGRAM_ID,
        })
        .remainingAccounts([
          { pubkey: mintKeypairs[0].publicKey, isWritable: true, isSigner: false },
          {
            pubkey: getAssociatedTokenAddressSync(
              mintKeypairs[0].publicKey,
              admin.publicKey,
              false,
              TOKEN_2022_PROGRAM_ID
            ),
            isWritable: true,
            isSigner: false,
          },
        ])
        .signers([badActor, admin.payer])
        .rpc();
      assert.fail("should have thrown Unauthorized");
    } catch (e: any) {
      assert.include(e.message, "Unauthorized");
    }
  });

  it("rejects invalid resource type", async () => {
    try {
      await program.methods
        .mintResources([6], [new BN(1)]) // type 6 is invalid
        .accounts({
          callerAuthority: mockSearchAuthority.publicKey,
          player: admin.publicKey,
          gameConfig,
          resourceAuthority,
          tokenProgram: TOKEN_2022_PROGRAM_ID,
          associatedTokenProgram: anchor.utils.token.ASSOCIATED_PROGRAM_ID,
          systemProgram: SystemProgram.programId,
        })
        .remainingAccounts([
          { pubkey: mintKeypairs[0].publicKey, isWritable: true, isSigner: false },
          {
            pubkey: getAssociatedTokenAddressSync(
              mintKeypairs[0].publicKey,
              admin.publicKey,
              false,
              TOKEN_2022_PROGRAM_ID
            ),
            isWritable: true,
            isSigner: false,
          },
        ])
        .signers([mockSearchAuthority])
        .rpc();
      assert.fail("should have thrown InvalidResourceType");
    } catch (e: any) {
      assert.include(e.message, "InvalidResourceType");
    }
  });
});
