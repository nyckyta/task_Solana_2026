import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { ItemNft } from "../target/types/item_nft";
import {
  TOKEN_PROGRAM_ID,
  getAssociatedTokenAddressSync,
  getAccount,
} from "@solana/spl-token";
import { Keypair, PublicKey, SystemProgram, SYSVAR_INSTRUCTIONS_PUBKEY } from "@solana/web3.js";
import { assert } from "chai";

const MPL_TOKEN_METADATA_PROGRAM_ID = new PublicKey(
  "metaqbxxUerdq28cj1RbAWkYQm3ybzjb6a8bt518x1s"
);

describe("item_nft", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);
  const program = anchor.workspace.ItemNft as Program<ItemNft>;
  const admin = provider.wallet as anchor.Wallet;

  const [config] = PublicKey.findProgramAddressSync(
    [Buffer.from("item_config")],
    program.programId
  );
  const [itemAuthority] = PublicKey.findProgramAddressSync(
    [Buffer.from("item_authority")],
    program.programId
  );

  const mockCraftingAuthority = Keypair.generate();
  const mockMarketplaceAuthority = Keypair.generate();

  let itemMintKp: Keypair;
  let itemMint: PublicKey;
  let player: Keypair;

  it("initializes item config", async () => {
    await program.methods
      .initialize()
      .accounts({
        admin: admin.publicKey,
        config,
        itemAuthority,
        systemProgram: SystemProgram.programId,
      })
      .rpc();

    const cfg = await program.account.itemConfig.fetch(config);
    assert.ok(cfg.admin.equals(admin.publicKey));
  });

  it("sets crafting and marketplace authorities", async () => {
    await program.methods
      .setAuthorities(
        mockCraftingAuthority.publicKey,
        mockMarketplaceAuthority.publicKey
      )
      .accounts({ admin: admin.publicKey, config })
      .rpc();

    const cfg = await program.account.itemConfig.fetch(config);
    assert.ok(cfg.craftingProgramAuthority.equals(mockCraftingAuthority.publicKey));
    assert.ok(cfg.marketplaceProgramAuthority.equals(mockMarketplaceAuthority.publicKey));
  });

  it("mints an NFT item when called by crafting authority", async () => {
    player = Keypair.generate();
    await provider.connection.confirmTransaction(
      await provider.connection.requestAirdrop(player.publicKey, 4e9)
    );

    itemMintKp = Keypair.generate();
    itemMint = itemMintKp.publicKey;

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
    const playerAta = getAssociatedTokenAddressSync(itemMint, player.publicKey);
    const [itemMetadataPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("item_metadata"), itemMint.toBuffer()],
      program.programId
    );

    await program.methods
      .mintItem(1) // ElderStick
      .accounts({
        callerAuthority: mockCraftingAuthority.publicKey,
        config,
        itemAuthority,
        mint: itemMint,
        metadata,
        masterEdition,
        player: player.publicKey,
        playerAta,
        itemMetadata: itemMetadataPda,
        feePayer: player.publicKey,
        tokenProgram: TOKEN_PROGRAM_ID,
        associatedTokenProgram: anchor.utils.token.ASSOCIATED_PROGRAM_ID,
        systemProgram: SystemProgram.programId,
        sysvarInstructions: SYSVAR_INSTRUCTIONS_PUBKEY,
        tokenMetadataProgram: MPL_TOKEN_METADATA_PROGRAM_ID,
      })
      .signers([mockCraftingAuthority, itemMintKp, player])
      .rpc();

    const ataInfo = await getAccount(provider.connection, playerAta);
    assert.equal(ataInfo.amount.toString(), "1");

    const meta = await program.account.itemMetadata.fetch(itemMetadataPda);
    assert.equal(meta.itemType, 1);
    assert.ok(meta.owner.equals(player.publicKey));
    assert.ok(meta.mint.equals(itemMint));
  });

  it("rejects mint from unauthorized caller", async () => {
    const badActor = Keypair.generate();
    const fakeItemMintKp = Keypair.generate();
    const fakeItemMint = fakeItemMintKp.publicKey;
    const [metadata] = PublicKey.findProgramAddressSync(
      [
        Buffer.from("metadata"),
        MPL_TOKEN_METADATA_PROGRAM_ID.toBuffer(),
        fakeItemMint.toBuffer(),
      ],
      MPL_TOKEN_METADATA_PROGRAM_ID
    );
    const [masterEdition] = PublicKey.findProgramAddressSync(
      [
        Buffer.from("metadata"),
        MPL_TOKEN_METADATA_PROGRAM_ID.toBuffer(),
        fakeItemMint.toBuffer(),
        Buffer.from("edition"),
      ],
      MPL_TOKEN_METADATA_PROGRAM_ID
    );
    const [itemMetadataPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("item_metadata"), fakeItemMint.toBuffer()],
      program.programId
    );

    try {
      await program.methods
        .mintItem(0)
        .accounts({
          callerAuthority: badActor.publicKey,
          config,
          itemAuthority,
          mint: fakeItemMint,
          metadata,
          masterEdition,
          player: admin.publicKey,
          playerAta: getAssociatedTokenAddressSync(fakeItemMint, admin.publicKey),
          itemMetadata: itemMetadataPda,
          feePayer: admin.publicKey,
          tokenProgram: TOKEN_PROGRAM_ID,
          associatedTokenProgram: anchor.utils.token.ASSOCIATED_PROGRAM_ID,
          systemProgram: SystemProgram.programId,
          sysvarInstructions: SYSVAR_INSTRUCTIONS_PUBKEY,
          tokenMetadataProgram: MPL_TOKEN_METADATA_PROGRAM_ID,
        })
        .signers([badActor, fakeItemMintKp])
        .rpc();
      assert.fail("should have thrown Unauthorized");
    } catch (e: any) {
      assert.include(e.message, "Unauthorized");
    }
  });

  it("burns an NFT item when called by marketplace authority", async () => {
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
    const holderAta = getAssociatedTokenAddressSync(itemMint, player.publicKey);
    const [itemMetadataPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("item_metadata"), itemMint.toBuffer()],
      program.programId
    );

    await program.methods
      .burnItem()
      .accounts({
        callerAuthority: mockMarketplaceAuthority.publicKey,
        config,
        itemAuthority,
        holder: player.publicKey,
        mint: itemMint,
        metadata,
        masterEdition,
        holderAta,
        itemMetadata: itemMetadataPda,
        tokenProgram: TOKEN_PROGRAM_ID,
        systemProgram: SystemProgram.programId,
        sysvarInstructions: SYSVAR_INSTRUCTIONS_PUBKEY,
        tokenMetadataProgram: MPL_TOKEN_METADATA_PROGRAM_ID,
      })
      .signers([mockMarketplaceAuthority, player])
      .rpc();

    // ATA should now have 0 balance
    const ataInfo = await getAccount(provider.connection, holderAta);
    assert.equal(ataInfo.amount.toString(), "0");

    // ItemMetadata PDA should be closed
    try {
      await program.account.itemMetadata.fetch(itemMetadataPda);
      assert.fail("item_metadata should be closed after burn");
    } catch (_) {}
  });

  it("rejects burn from unauthorized caller", async () => {
    const badActor = Keypair.generate();
    // Use previously burned mint — the accounts don't exist, so it will fail at constraint level
    try {
      await program.methods
        .burnItem()
        .accounts({
          callerAuthority: badActor.publicKey,
          config,
          itemAuthority,
          holder: player.publicKey,
          mint: itemMint,
          metadata: player.publicKey, // dummy — test should fail before reaching CPI
          masterEdition: player.publicKey,
          holderAta: getAssociatedTokenAddressSync(itemMint, player.publicKey),
          itemMetadata: player.publicKey,
          tokenProgram: TOKEN_PROGRAM_ID,
          systemProgram: SystemProgram.programId,
          sysvarInstructions: SYSVAR_INSTRUCTIONS_PUBKEY,
          tokenMetadataProgram: MPL_TOKEN_METADATA_PROGRAM_ID,
        })
        .signers([badActor, player])
        .rpc();
      assert.fail("should have thrown Unauthorized");
    } catch (e: any) {
      assert.include(e.message, "Unauthorized");
    }
  });

  it("rejects invalid item type", async () => {
    const fakeMintKp = Keypair.generate();
    const fakeMint = fakeMintKp.publicKey;
    const [metadata] = PublicKey.findProgramAddressSync(
      [
        Buffer.from("metadata"),
        MPL_TOKEN_METADATA_PROGRAM_ID.toBuffer(),
        fakeMint.toBuffer(),
      ],
      MPL_TOKEN_METADATA_PROGRAM_ID
    );
    const [masterEdition] = PublicKey.findProgramAddressSync(
      [
        Buffer.from("metadata"),
        MPL_TOKEN_METADATA_PROGRAM_ID.toBuffer(),
        fakeMint.toBuffer(),
        Buffer.from("edition"),
      ],
      MPL_TOKEN_METADATA_PROGRAM_ID
    );
    const [itemMetadataPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("item_metadata"), fakeMint.toBuffer()],
      program.programId
    );

    try {
      await program.methods
        .mintItem(99)
        .accounts({
          callerAuthority: mockCraftingAuthority.publicKey,
          config,
          itemAuthority,
          mint: fakeMint,
          metadata,
          masterEdition,
          player: admin.publicKey,
          playerAta: getAssociatedTokenAddressSync(fakeMint, admin.publicKey),
          itemMetadata: itemMetadataPda,
          feePayer: admin.publicKey,
          tokenProgram: TOKEN_PROGRAM_ID,
          associatedTokenProgram: anchor.utils.token.ASSOCIATED_PROGRAM_ID,
          systemProgram: SystemProgram.programId,
          sysvarInstructions: SYSVAR_INSTRUCTIONS_PUBKEY,
          tokenMetadataProgram: MPL_TOKEN_METADATA_PROGRAM_ID,
        })
        .signers([mockCraftingAuthority, fakeMintKp])
        .rpc();
      assert.fail("should have thrown InvalidItemType");
    } catch (e: any) {
      assert.include(e.message, "InvalidItemType");
    }
  });
});
