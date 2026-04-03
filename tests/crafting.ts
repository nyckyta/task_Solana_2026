import * as anchor from "@coral-xyz/anchor";
import { Program, BN } from "@coral-xyz/anchor";
import { Crafting } from "../target/types/crafting";
import { ResourceManager } from "../target/types/resource_manager";
import { ItemNft } from "../target/types/item_nft";
import {
  TOKEN_2022_PROGRAM_ID,
  TOKEN_PROGRAM_ID,
  getAssociatedTokenAddressSync,
  getAccount,
  createAssociatedTokenAccountInstruction,
} from "@solana/spl-token";
import { Keypair, PublicKey, SystemProgram, SYSVAR_INSTRUCTIONS_PUBKEY } from "@solana/web3.js";
import { Metadata, MasterEdition } from "@metaplex-foundation/mpl-token-metadata";
import { assert } from "chai";

const MPL_TOKEN_METADATA_PROGRAM_ID = new PublicKey(
  "metaqbxxUerdq28cj1RbAWkYQm3ybzjb6a8bt518x1s"
);

describe("crafting", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const craftingProgram = anchor.workspace.Crafting as Program<Crafting>;
  const resourceProgram = anchor.workspace.ResourceManager as Program<ResourceManager>;
  const itemNftProgram = anchor.workspace.ItemNft as Program<ItemNft>;
  const admin = provider.wallet as anchor.Wallet;

  const [craftingConfig] = PublicKey.findProgramAddressSync(
    [Buffer.from("crafting_config")],
    craftingProgram.programId
  );
  const [craftingAuthority] = PublicKey.findProgramAddressSync(
    [Buffer.from("crafting_authority")],
    craftingProgram.programId
  );
  const [resourceGameConfig] = PublicKey.findProgramAddressSync(
    [Buffer.from("game_config")],
    resourceProgram.programId
  );
  const [itemConfig] = PublicKey.findProgramAddressSync(
    [Buffer.from("item_config")],
    itemNftProgram.programId
  );
  const [itemAuthority] = PublicKey.findProgramAddressSync(
    [Buffer.from("item_authority")],
    itemNftProgram.programId
  );
  const [resourceAuthority] = PublicKey.findProgramAddressSync(
    [Buffer.from("resource_authority")],
    resourceProgram.programId
  );
  const mockSearchAuthority = Keypair.generate();
  const mockCraftingAuthorityForSearch = craftingAuthority; // crafting authority in resource_manager

  let resourceMints: PublicKey[];
  let player: Keypair;

  before(async () => {
    player = Keypair.generate();
    await provider.connection.confirmTransaction(
      await provider.connection.requestAirdrop(player.publicKey, 4e9)
    );

    const cfg = await resourceProgram.account.gameConfig.fetch(resourceGameConfig);
    resourceMints = cfg.resourceMints as PublicKey[];

    // Initialize crafting config
    await craftingProgram.methods
      .initialize()
      .accounts({
        admin: admin.publicKey,
        config: craftingConfig,
        craftingAuthority,
        systemProgram: SystemProgram.programId,
      })
      .rpc();

    // Initialize item_nft config and set crafting authority
    await itemNftProgram.methods
      .initialize()
      .accounts({
        admin: admin.publicKey,
        config: itemConfig,
        itemAuthority,
        systemProgram: SystemProgram.programId,
      })
      .rpc();

    await itemNftProgram.methods
      .setAuthorities(craftingAuthority, Keypair.generate().publicKey)
      .accounts({ admin: admin.publicKey, config: itemConfig })
      .rpc();

    // Set crafting authority in resource_manager
    await resourceProgram.methods
      .setAuthorities(mockSearchAuthority.publicKey, craftingAuthority)
      .accounts({ admin: admin.publicKey, gameConfig: resourceGameConfig })
      .rpc();

    // Give player resources for crafting a KozakSable: 3×Iron(1) + 1×Wood(0) + 1×Leather(3)
    const ironMint = resourceMints[1];
    const woodMint = resourceMints[0];
    const leatherMint = resourceMints[3];

    for (const [mint, amount] of [
      [ironMint, 3],
      [woodMint, 1],
      [leatherMint, 1],
    ] as [PublicKey, number][]) {
      const ata = getAssociatedTokenAddressSync(
        mint,
        player.publicKey,
        false,
        TOKEN_2022_PROGRAM_ID
      );
      const createAtaIx = createAssociatedTokenAccountInstruction(
        player.publicKey,
        ata,
        player.publicKey,
        mint,
        TOKEN_2022_PROGRAM_ID
      );
      await provider.sendAndConfirm(
        new anchor.web3.Transaction().add(createAtaIx),
        [player]
      );

      await resourceProgram.methods
        .mintResources([resourceMints.indexOf(mint)], [new BN(amount)])
        .accounts({
          callerAuthority: mockSearchAuthority.publicKey,
          player: player.publicKey,
          gameConfig: resourceGameConfig,
          resourceAuthority,
          tokenProgram: TOKEN_2022_PROGRAM_ID,
          associatedTokenProgram: anchor.utils.token.ASSOCIATED_PROGRAM_ID,
          systemProgram: SystemProgram.programId,
        })
        .remainingAccounts([
          { pubkey: mint, isWritable: true, isSigner: false },
          { pubkey: ata, isWritable: true, isSigner: false },
        ])
        .signers([mockSearchAuthority])
        .rpc();
    }
  });

  it("crafts a KozakSable (item type 0)", async () => {
    const itemMintKp = Keypair.generate();
    const itemMint = itemMintKp.publicKey;

    const [metadata] = PublicKey.findProgramAddressSync(
      [
        Buffer.from("metadata"),
        MPL_TOKEN_METADATA_PROGRAM_ID.toBuffer(),
        itemMint.toBuffer(),
      ],
      MPL_TOKEN_METADATA_PROGRAM_ID
    );
    const [masterEdition] = PublicKey.findProgramAddressSync(
      [
        Buffer.from("metadata"),
        MPL_TOKEN_METADATA_PROGRAM_ID.toBuffer(),
        itemMint.toBuffer(),
        Buffer.from("edition"),
      ],
      MPL_TOKEN_METADATA_PROGRAM_ID
    );
    const playerItemAta = getAssociatedTokenAddressSync(itemMint, player.publicKey);
    const [itemMetadataPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("item_metadata"), itemMint.toBuffer()],
      itemNftProgram.programId
    );

    // Recipe for KozakSable: [Iron=1, Wood=0, Leather=3], [3, 1, 1]
    const ironAta = getAssociatedTokenAddressSync(
      resourceMints[1],
      player.publicKey,
      false,
      TOKEN_2022_PROGRAM_ID
    );
    const woodAta = getAssociatedTokenAddressSync(
      resourceMints[0],
      player.publicKey,
      false,
      TOKEN_2022_PROGRAM_ID
    );
    const leatherAta = getAssociatedTokenAddressSync(
      resourceMints[3],
      player.publicKey,
      false,
      TOKEN_2022_PROGRAM_ID
    );

    // remaining_accounts layout:
    //   [iron_mint, iron_ata, wood_mint, wood_ata, leather_mint, leather_ata,  <- resources (n=3 pairs)
    //    item_mint, player_item_ata, metadata, master_edition,                  <- NFT
    //    item_metadata_pda,                                                      <- our PDA
    //    sysvar_instructions, token_metadata_program]
    const remaining: anchor.web3.AccountMeta[] = [
      { pubkey: resourceMints[1], isWritable: true, isSigner: false },
      { pubkey: ironAta, isWritable: true, isSigner: false },
      { pubkey: resourceMints[0], isWritable: true, isSigner: false },
      { pubkey: woodAta, isWritable: true, isSigner: false },
      { pubkey: resourceMints[3], isWritable: true, isSigner: false },
      { pubkey: leatherAta, isWritable: true, isSigner: false },
      { pubkey: itemMint, isWritable: true, isSigner: true },
      { pubkey: playerItemAta, isWritable: true, isSigner: false },
      { pubkey: metadata, isWritable: true, isSigner: false },
      { pubkey: masterEdition, isWritable: true, isSigner: false },
      { pubkey: itemMetadataPda, isWritable: true, isSigner: false },
      { pubkey: SYSVAR_INSTRUCTIONS_PUBKEY, isWritable: false, isSigner: false },
      { pubkey: MPL_TOKEN_METADATA_PROGRAM_ID, isWritable: false, isSigner: false },
    ];

    await craftingProgram.methods
      .craftItem(0)
      .accounts({
        player: player.publicKey,
        config: craftingConfig,
        craftingAuthority,
        resourceGameConfig,
        resourceManagerProgram: resourceProgram.programId,
        itemConfig,
        itemAuthority,
        itemNftProgram: itemNftProgram.programId,
        tokenProgram: TOKEN_2022_PROGRAM_ID,
        splTokenProgram: TOKEN_PROGRAM_ID,
        associatedTokenProgram: anchor.utils.token.ASSOCIATED_PROGRAM_ID,
        systemProgram: SystemProgram.programId,
      })
      .remainingAccounts(remaining)
      .signers([player, itemMintKp])
      .rpc();

    // Verify resources burned
    const ironInfo = await getAccount(
      provider.connection,
      ironAta,
      undefined,
      TOKEN_2022_PROGRAM_ID
    );
    assert.equal(ironInfo.amount.toString(), "0", "Iron should be burned");

    // Verify NFT minted
    const itemInfo = await getAccount(provider.connection, playerItemAta);
    assert.equal(itemInfo.amount.toString(), "1", "Player should hold the NFT");

    // Verify ItemMetadata PDA
    const itemMeta = await itemNftProgram.account.itemMetadata.fetch(itemMetadataPda);
    assert.equal(itemMeta.itemType, 0, "Item type should be 0 (KozakSable)");
    assert.ok(itemMeta.owner.equals(player.publicKey));
  });

  it("fails crafting with insufficient resources", async () => {
    // Player has 0 resources left after previous craft
    const itemMintKp = Keypair.generate();
    const itemMint = itemMintKp.publicKey;
    const [metadata] = PublicKey.findProgramAddressSync(
      [
        Buffer.from("metadata"),
        MPL_TOKEN_METADATA_PROGRAM_ID.toBuffer(),
        itemMint.toBuffer(),
      ],
      MPL_TOKEN_METADATA_PROGRAM_ID
    );
    const [masterEdition] = PublicKey.findProgramAddressSync(
      [
        Buffer.from("metadata"),
        MPL_TOKEN_METADATA_PROGRAM_ID.toBuffer(),
        itemMint.toBuffer(),
        Buffer.from("edition"),
      ],
      MPL_TOKEN_METADATA_PROGRAM_ID
    );
    const playerItemAta = getAssociatedTokenAddressSync(itemMint, player.publicKey);
    const [itemMetadataPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("item_metadata"), itemMint.toBuffer()],
      itemNftProgram.programId
    );

    const ironAta = getAssociatedTokenAddressSync(
      resourceMints[1],
      player.publicKey,
      false,
      TOKEN_2022_PROGRAM_ID
    );
    const woodAta = getAssociatedTokenAddressSync(
      resourceMints[0],
      player.publicKey,
      false,
      TOKEN_2022_PROGRAM_ID
    );
    const leatherAta = getAssociatedTokenAddressSync(
      resourceMints[3],
      player.publicKey,
      false,
      TOKEN_2022_PROGRAM_ID
    );

    try {
      await craftingProgram.methods
        .craftItem(0)
        .accounts({
          player: player.publicKey,
          config: craftingConfig,
          craftingAuthority,
          resourceGameConfig,
          resourceManagerProgram: resourceProgram.programId,
          itemConfig,
          itemAuthority,
          itemNftProgram: itemNftProgram.programId,
          tokenProgram: TOKEN_2022_PROGRAM_ID,
          splTokenProgram: TOKEN_PROGRAM_ID,
          associatedTokenProgram: anchor.utils.token.ASSOCIATED_PROGRAM_ID,
          systemProgram: SystemProgram.programId,
        })
        .remainingAccounts([
          { pubkey: resourceMints[1], isWritable: true, isSigner: false },
          { pubkey: ironAta, isWritable: true, isSigner: false },
          { pubkey: resourceMints[0], isWritable: true, isSigner: false },
          { pubkey: woodAta, isWritable: true, isSigner: false },
          { pubkey: resourceMints[3], isWritable: true, isSigner: false },
          { pubkey: leatherAta, isWritable: true, isSigner: false },
          { pubkey: itemMint, isWritable: true, isSigner: true },
          { pubkey: playerItemAta, isWritable: true, isSigner: false },
          { pubkey: metadata, isWritable: true, isSigner: false },
          { pubkey: masterEdition, isWritable: true, isSigner: false },
          { pubkey: itemMetadataPda, isWritable: true, isSigner: false },
          { pubkey: SYSVAR_INSTRUCTIONS_PUBKEY, isWritable: false, isSigner: false },
          { pubkey: MPL_TOKEN_METADATA_PROGRAM_ID, isWritable: false, isSigner: false },
        ])
        .signers([player, itemMintKp])
        .rpc();
      assert.fail("should have failed due to insufficient resources");
    } catch (_) {
      // Expected: Token program will throw insufficient funds
    }
  });

  it("fails crafting with invalid item type", async () => {
    try {
      await craftingProgram.methods
        .craftItem(99)
        .accounts({
          player: player.publicKey,
          config: craftingConfig,
          craftingAuthority,
          resourceGameConfig,
          resourceManagerProgram: resourceProgram.programId,
          itemConfig,
          itemAuthority,
          itemNftProgram: itemNftProgram.programId,
          tokenProgram: TOKEN_2022_PROGRAM_ID,
          splTokenProgram: TOKEN_PROGRAM_ID,
          associatedTokenProgram: anchor.utils.token.ASSOCIATED_PROGRAM_ID,
          systemProgram: SystemProgram.programId,
        })
        .remainingAccounts([])
        .signers([player])
        .rpc();
      assert.fail("should have thrown InvalidItemType");
    } catch (e: any) {
      assert.include(e.message, "InvalidItemType");
    }
  });
});
