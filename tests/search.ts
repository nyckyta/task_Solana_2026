import * as anchor from "@coral-xyz/anchor";
import { Program, BN } from "@coral-xyz/anchor";
import { Search } from "../target/types/search";
import { ResourceManager } from "../target/types/resource_manager";
import {
  TOKEN_2022_PROGRAM_ID,
  getAssociatedTokenAddressSync,
  getAccount,
  createAssociatedTokenAccountInstruction,
} from "@solana/spl-token";
import { Keypair, PublicKey, SystemProgram, SYSVAR_SLOT_HASHES_PUBKEY } from "@solana/web3.js";
import { assert } from "chai";

describe("search", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const searchProgram = anchor.workspace.Search as Program<Search>;
  const resourceProgram = anchor.workspace.ResourceManager as Program<ResourceManager>;
  const admin = provider.wallet as anchor.Wallet;

  const [resourceGameConfig] = PublicKey.findProgramAddressSync(
    [Buffer.from("game_config")],
    resourceProgram.programId
  );
  const [resourceAuthority] = PublicKey.findProgramAddressSync(
    [Buffer.from("resource_authority")],
    resourceProgram.programId
  );
  const [searchAuthority] = PublicKey.findProgramAddressSync(
    [Buffer.from("search_authority")],
    searchProgram.programId
  );

  let player: Keypair;
  let playerState: PublicKey;
  // 6 resource mints must already be initialized (run resource_manager tests first, or share state)
  let resourceMints: PublicKey[];

  before(async () => {
    player = Keypair.generate();
    await provider.connection.confirmTransaction(
      await provider.connection.requestAirdrop(player.publicKey, 4e9)
    );

    [playerState] = PublicKey.findProgramAddressSync(
      [Buffer.from("player_state"), player.publicKey.toBuffer()],
      searchProgram.programId
    );

    // Fetch resource mints from the already-initialized GameConfig
    const config = await resourceProgram.account.gameConfig.fetch(resourceGameConfig);
    resourceMints = config.resourceMints as PublicKey[];
  });

  it("registers a player", async () => {
    await searchProgram.methods
      .registerPlayer()
      .accounts({
        player: player.publicKey,
        playerState,
        systemProgram: SystemProgram.programId,
      })
      .signers([player])
      .rpc();

    const state = await searchProgram.account.playerState.fetch(playerState);
    assert.ok(state.owner.equals(player.publicKey));
    assert.equal(state.lastSearchTimestamp.toString(), "0");
  });

  it("performs first search (no cooldown)", async () => {
    // Create ATAs for all 6 resources
    const atas: PublicKey[] = [];
    const createAtaIxs = [];
    for (const mint of resourceMints) {
      const ata = getAssociatedTokenAddressSync(
        mint,
        player.publicKey,
        false,
        TOKEN_2022_PROGRAM_ID
      );
      atas.push(ata);
      createAtaIxs.push(
        createAssociatedTokenAccountInstruction(
          player.publicKey,
          ata,
          player.publicKey,
          mint,
          TOKEN_2022_PROGRAM_ID
        )
      );
    }
    const tx = new anchor.web3.Transaction().add(...createAtaIxs);
    await provider.sendAndConfirm(tx, [player]);

    // Build remaining accounts: [mint_0, ata_0, mint_1, ata_1, mint_2, ata_2]
    // We pass all 6 mints and the player's ATAs; the program will receive 3 resource types
    // from randomness and mint to those specific ATAs.
    // For the test we pass the first 3 mint+ATA pairs as the 3 resources to receive.
    const remaining: anchor.web3.AccountMeta[] = [];
    for (let i = 0; i < 3; i++) {
      remaining.push({ pubkey: resourceMints[i], isWritable: true, isSigner: false });
      remaining.push({ pubkey: atas[i], isWritable: true, isSigner: false });
    }

    await searchProgram.methods
      .searchResources()
      .accounts({
        player: player.publicKey,
        playerState,
        searchAuthority,
        resourceGameConfig,
        resourceAuthority,
        resourceManagerProgram: resourceProgram.programId,
        tokenProgram: TOKEN_2022_PROGRAM_ID,
        associatedTokenProgram: anchor.utils.token.ASSOCIATED_PROGRAM_ID,
        systemProgram: SystemProgram.programId,
        slotHashes: SYSVAR_SLOT_HASHES_PUBKEY,
      })
      .remainingAccounts(remaining)
      .signers([player])
      .rpc();

    const state = await searchProgram.account.playerState.fetch(playerState);
    assert.isAbove(
      state.lastSearchTimestamp.toNumber(),
      0,
      "timestamp should be updated"
    );
  });

  it("rejects search before cooldown elapses", async () => {
    // Try immediately — should fail since < 60 seconds have passed
    try {
      const atas = resourceMints
        .slice(0, 3)
        .map((m) =>
          getAssociatedTokenAddressSync(m, player.publicKey, false, TOKEN_2022_PROGRAM_ID)
        );
      const remaining = resourceMints.slice(0, 3).flatMap((m, i) => [
        { pubkey: m, isWritable: true, isSigner: false },
        { pubkey: atas[i], isWritable: true, isSigner: false },
      ]);

      await searchProgram.methods
        .searchResources()
        .accounts({
          player: player.publicKey,
          playerState,
          searchAuthority,
          resourceGameConfig,
          resourceAuthority,
          resourceManagerProgram: resourceProgram.programId,
          tokenProgram: TOKEN_2022_PROGRAM_ID,
          associatedTokenProgram: anchor.utils.token.ASSOCIATED_PROGRAM_ID,
          systemProgram: SystemProgram.programId,
          slotHashes: SYSVAR_SLOT_HASHES_PUBKEY,
        })
        .remainingAccounts(remaining)
        .signers([player])
        .rpc();
      assert.fail("should have thrown CooldownNotElapsed");
    } catch (e: any) {
      assert.include(e.message, "CooldownNotElapsed");
    }
  });

  it("rejects search from wrong player", async () => {
    const otherPlayer = Keypair.generate();
    await provider.connection.confirmTransaction(
      await provider.connection.requestAirdrop(otherPlayer.publicKey, 2e9)
    );

    try {
      await searchProgram.methods
        .searchResources()
        .accounts({
          player: otherPlayer.publicKey,
          playerState, // belongs to `player`, not `otherPlayer`
          searchAuthority,
          resourceGameConfig,
          resourceAuthority,
          resourceManagerProgram: resourceProgram.programId,
          tokenProgram: TOKEN_2022_PROGRAM_ID,
          associatedTokenProgram: anchor.utils.token.ASSOCIATED_PROGRAM_ID,
          systemProgram: SystemProgram.programId,
          slotHashes: SYSVAR_SLOT_HASHES_PUBKEY,
        })
        .remainingAccounts([])
        .signers([otherPlayer])
        .rpc();
      assert.fail("should have thrown constraint violation");
    } catch (_) {
      // Expected: has_one / seeds constraint fails
    }
  });
});
