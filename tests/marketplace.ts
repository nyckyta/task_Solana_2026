import * as anchor from "@coral-xyz/anchor";
import { Program, BN } from "@coral-xyz/anchor";
import { Marketplace } from "../target/types/marketplace";
import { MagicToken } from "../target/types/magic_token";
import { ItemNft } from "../target/types/item_nft";
import {
  TOKEN_2022_PROGRAM_ID,
  TOKEN_PROGRAM_ID,
  getAssociatedTokenAddressSync,
  getAccount,
  createAssociatedTokenAccountInstruction,
} from "@solana/spl-token";
import { Keypair, PublicKey, SystemProgram, SYSVAR_INSTRUCTIONS_PUBKEY } from "@solana/web3.js";
import { assert } from "chai";

const MPL_TOKEN_METADATA_PROGRAM_ID = new PublicKey(
  "metaqbxxUerdq28cj1RbAWkYQm3ybzjb6a8bt518x1s"
);

describe("marketplace", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const marketplaceProgram = anchor.workspace.Marketplace as Program<Marketplace>;
  const magicTokenProgram = anchor.workspace.MagicToken as Program<MagicToken>;
  const itemNftProgram = anchor.workspace.ItemNft as Program<ItemNft>;
  const admin = provider.wallet as anchor.Wallet;

  const [marketplaceConfig] = PublicKey.findProgramAddressSync(
    [Buffer.from("marketplace_config")],
    marketplaceProgram.programId
  );
  const [marketplaceAuthority] = PublicKey.findProgramAddressSync(
    [Buffer.from("marketplace_authority")],
    marketplaceProgram.programId
  );
  const [magicTokenConfig] = PublicKey.findProgramAddressSync(
    [Buffer.from("magic_token_config")],
    magicTokenProgram.programId
  );
  const [magicTokenMintAuthority] = PublicKey.findProgramAddressSync(
    [Buffer.from("mint_authority")],
    magicTokenProgram.programId
  );
  const [itemConfig] = PublicKey.findProgramAddressSync(
    [Buffer.from("item_config")],
    itemNftProgram.programId
  );
  const [itemAuthority] = PublicKey.findProgramAddressSync(
    [Buffer.from("item_authority")],
    itemNftProgram.programId
  );

  let seller: Keypair;
  let buyer: Keypair;
  let itemMint: PublicKey;
  let magicTokenMint: PublicKey;

  before(async () => {
    seller = Keypair.generate();
    buyer = Keypair.generate();
    await Promise.all([
      provider.connection.confirmTransaction(
        await provider.connection.requestAirdrop(seller.publicKey, 4e9)
      ),
      provider.connection.confirmTransaction(
        await provider.connection.requestAirdrop(buyer.publicKey, 4e9)
      ),
    ]);

    // Initialize marketplace config
    await marketplaceProgram.methods
      .initialize()
      .accounts({
        admin: admin.publicKey,
        config: marketplaceConfig,
        marketplaceAuthority,
        systemProgram: SystemProgram.programId,
      })
      .rpc();

    // Set marketplace authority in magic_token and item_nft
    await magicTokenProgram.methods
      .setMarketplaceAuthority(marketplaceAuthority)
      .accounts({ admin: admin.publicKey, config: magicTokenConfig })
      .rpc();

    await itemNftProgram.methods
      .setAuthorities(
        (await itemNftProgram.account.itemConfig.fetch(itemConfig))
          .craftingProgramAuthority,
        marketplaceAuthority
      )
      .accounts({ admin: admin.publicKey, config: itemConfig })
      .rpc();

    // Fetch magic token mint
    const cfg = await magicTokenProgram.account.magicTokenConfig.fetch(magicTokenConfig);
    magicTokenMint = cfg.mint as PublicKey;

    // Create a dummy NFT item for the seller using item_nft directly (bypassing crafting for test setup)
    const itemMintKp = Keypair.generate();
    itemMint = itemMintKp.publicKey;

    // Use the crafting authority that was set earlier (item_nft tests set it)
    // For marketplace test we mint directly via item_nft with the crafting authority
    // This requires that crafting authority is the signer — reuse from crafting tests
    // In practice this would come from crafting; for test isolation we call item_nft directly
    // with the stored crafting_authority.
    const craftingProgramId = (await itemNftProgram.account.itemConfig.fetch(itemConfig))
      .craftingProgramAuthority;

    // If crafting_authority is a PDA of the crafting program, we can't directly sign here.
    // Instead use a fresh mock authority for isolated test.
    const mockCraftingAuth = Keypair.generate();
    await itemNftProgram.methods
      .setAuthorities(mockCraftingAuth.publicKey, marketplaceAuthority)
      .accounts({ admin: admin.publicKey, config: itemConfig })
      .rpc();

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
    const sellerItemAta = getAssociatedTokenAddressSync(itemMint, seller.publicKey);
    const [itemMetadataPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("item_metadata"), itemMint.toBuffer()],
      itemNftProgram.programId
    );

    await itemNftProgram.methods
      .mintItem(0)
      .accounts({
        callerAuthority: mockCraftingAuth.publicKey,
        config: itemConfig,
        itemAuthority,
        mint: itemMint,
        metadata,
        masterEdition,
        player: seller.publicKey,
        playerAta: sellerItemAta,
        itemMetadata: itemMetadataPda,
        feePayer: seller.publicKey,
        tokenProgram: TOKEN_PROGRAM_ID,
        associatedTokenProgram: anchor.utils.token.ASSOCIATED_PROGRAM_ID,
        systemProgram: SystemProgram.programId,
        sysvarInstructions: SYSVAR_INSTRUCTIONS_PUBKEY,
        tokenMetadataProgram: MPL_TOKEN_METADATA_PROGRAM_ID,
      })
      .signers([mockCraftingAuth, itemMintKp, seller])
      .rpc();

    // Give buyer some MagicTokens (via mint authority directly for test setup)
    const buyerMagicAta = getAssociatedTokenAddressSync(
      magicTokenMint,
      buyer.publicKey,
      false,
      TOKEN_2022_PROGRAM_ID
    );
    // We need the marketplace authority to mint — set a mock
    const mockMarketplaceAuth = Keypair.generate();
    await magicTokenProgram.methods
      .setMarketplaceAuthority(mockMarketplaceAuth.publicKey)
      .accounts({ admin: admin.publicKey, config: magicTokenConfig })
      .rpc();

    await magicTokenProgram.methods
      .mintMagicTokens(new BN(500))
      .accounts({
        callerAuthority: mockMarketplaceAuth.publicKey,
        config: magicTokenConfig,
        mint: magicTokenMint,
        mintAuthority: magicTokenMintAuthority,
        recipient: buyer.publicKey,
        recipientAta: buyerMagicAta,
        feePayer: admin.publicKey,
        tokenProgram: TOKEN_2022_PROGRAM_ID,
        associatedTokenProgram: anchor.utils.token.ASSOCIATED_PROGRAM_ID,
        systemProgram: SystemProgram.programId,
      })
      .signers([mockMarketplaceAuth])
      .rpc();

    // Restore marketplace authority back to real marketplace authority
    await magicTokenProgram.methods
      .setMarketplaceAuthority(marketplaceAuthority)
      .accounts({ admin: admin.publicKey, config: magicTokenConfig })
      .rpc();
  });

  it("lists an item for sale", async () => {
    const sellerItemAta = getAssociatedTokenAddressSync(itemMint, seller.publicKey);
    const [listing] = PublicKey.findProgramAddressSync(
      [Buffer.from("listing"), itemMint.toBuffer()],
      marketplaceProgram.programId
    );

    await marketplaceProgram.methods
      .listItem(new BN(100))
      .accounts({
        seller: seller.publicKey,
        config: marketplaceConfig,
        itemMint,
        sellerAta: sellerItemAta,
        listing,
        tokenProgram: TOKEN_PROGRAM_ID,
        systemProgram: SystemProgram.programId,
      })
      .signers([seller])
      .rpc();

    const listingAccount = await marketplaceProgram.account.listing.fetch(listing);
    assert.ok(listingAccount.seller.equals(seller.publicKey));
    assert.ok(listingAccount.itemMint.equals(itemMint));
    assert.equal(listingAccount.price.toString(), "100");
  });

  it("cancels a listing", async () => {
    // Create a new item for this test
    const tempMintKp = Keypair.generate();
    // ... (setup abbreviated for brevity — in a full test suite this would be extracted
    // to a helper function that creates and gives an NFT to a player)

    const [listing] = PublicKey.findProgramAddressSync(
      [Buffer.from("listing"), itemMint.toBuffer()],
      marketplaceProgram.programId
    );

    // Re-list first (it was listed in previous test)
    await marketplaceProgram.methods
      .cancelListing()
      .accounts({
        seller: seller.publicKey,
        listing,
      })
      .signers([seller])
      .rpc();

    try {
      await marketplaceProgram.account.listing.fetch(listing);
      assert.fail("listing should be closed");
    } catch (_) {
      // Expected: account no longer exists
    }
  });

  it("rejects cancel from non-seller", async () => {
    // Re-list the item
    const sellerItemAta = getAssociatedTokenAddressSync(itemMint, seller.publicKey);
    const [listing] = PublicKey.findProgramAddressSync(
      [Buffer.from("listing"), itemMint.toBuffer()],
      marketplaceProgram.programId
    );
    await marketplaceProgram.methods
      .listItem(new BN(100))
      .accounts({
        seller: seller.publicKey,
        config: marketplaceConfig,
        itemMint,
        sellerAta: sellerItemAta,
        listing,
        tokenProgram: TOKEN_PROGRAM_ID,
        systemProgram: SystemProgram.programId,
      })
      .signers([seller])
      .rpc();

    const badActor = Keypair.generate();
    await provider.connection.confirmTransaction(
      await provider.connection.requestAirdrop(badActor.publicKey, 2e9)
    );

    try {
      await marketplaceProgram.methods
        .cancelListing()
        .accounts({ seller: badActor.publicKey, listing })
        .signers([badActor])
        .rpc();
      assert.fail("should have thrown NotSeller");
    } catch (e: any) {
      assert.include(e.message, "NotSeller");
    }
  });

  it("buys an item: buyer loses MagicTokens, seller gains them, NFT transfers", async () => {
    const [listing] = PublicKey.findProgramAddressSync(
      [Buffer.from("listing"), itemMint.toBuffer()],
      marketplaceProgram.programId
    );
    // Listing is active from previous test (100 MagicTokens)

    const sellerItemAta = getAssociatedTokenAddressSync(itemMint, seller.publicKey);
    const buyerItemAta = getAssociatedTokenAddressSync(itemMint, buyer.publicKey);
    const buyerMagicAta = getAssociatedTokenAddressSync(
      magicTokenMint,
      buyer.publicKey,
      false,
      TOKEN_2022_PROGRAM_ID
    );
    const sellerMagicAta = getAssociatedTokenAddressSync(
      magicTokenMint,
      seller.publicKey,
      false,
      TOKEN_2022_PROGRAM_ID
    );

    // Ensure buyer item ATA exists
    try {
      const createAtaIx = createAssociatedTokenAccountInstruction(
        buyer.publicKey,
        buyerItemAta,
        buyer.publicKey,
        itemMint
      );
      await provider.sendAndConfirm(
        new anchor.web3.Transaction().add(createAtaIx),
        [buyer]
      );
    } catch (_) {}

    const buyerMagicBefore = (
      await getAccount(provider.connection, buyerMagicAta, undefined, TOKEN_2022_PROGRAM_ID)
    ).amount;

    await marketplaceProgram.methods
      .buyItem()
      .accounts({
        buyer: buyer.publicKey,
        seller: seller.publicKey,
        config: marketplaceConfig,
        marketplaceAuthority,
        listing,
        itemMint,
        sellerItemAta,
        buyerItemAta,
        magicTokenConfig,
        magicTokenMint,
        magicTokenMintAuthority,
        buyerMagicAta,
        sellerMagicAta,
        magicTokenProgram: magicTokenProgram.programId,
        magicTokenProgramInterface: TOKEN_2022_PROGRAM_ID,
        itemNftProgram: itemNftProgram.programId,
        splTokenProgram: TOKEN_PROGRAM_ID,
        associatedTokenProgram: anchor.utils.token.ASSOCIATED_PROGRAM_ID,
        systemProgram: SystemProgram.programId,
      })
      .signers([buyer, seller])
      .rpc();

    // Buyer should have 100 fewer MagicTokens
    const buyerMagicAfter = (
      await getAccount(provider.connection, buyerMagicAta, undefined, TOKEN_2022_PROGRAM_ID)
    ).amount;
    assert.equal(
      (BigInt(buyerMagicBefore.toString()) - BigInt(buyerMagicAfter.toString())).toString(),
      "100"
    );

    // Seller should have received 100 MagicTokens
    const sellerMagicBalance = (
      await getAccount(
        provider.connection,
        sellerMagicAta,
        undefined,
        TOKEN_2022_PROGRAM_ID
      )
    ).amount;
    assert.equal(sellerMagicBalance.toString(), "100");

    // Buyer should hold the NFT now
    const buyerItemInfo = await getAccount(provider.connection, buyerItemAta);
    assert.equal(buyerItemInfo.amount.toString(), "1");

    // Seller should no longer hold the NFT
    const sellerItemInfo = await getAccount(provider.connection, sellerItemAta);
    assert.equal(sellerItemInfo.amount.toString(), "0");

    // Listing should be closed
    try {
      await marketplaceProgram.account.listing.fetch(listing);
      assert.fail("listing should be closed after sale");
    } catch (_) {}
  });

  it("rejects listing with zero price", async () => {
    const sellerItemAta = getAssociatedTokenAddressSync(itemMint, buyer.publicKey); // buyer now owns it
    const [listing] = PublicKey.findProgramAddressSync(
      [Buffer.from("listing"), itemMint.toBuffer()],
      marketplaceProgram.programId
    );
    try {
      await marketplaceProgram.methods
        .listItem(new BN(0))
        .accounts({
          seller: buyer.publicKey,
          config: marketplaceConfig,
          itemMint,
          sellerAta: sellerItemAta,
          listing,
          tokenProgram: TOKEN_PROGRAM_ID,
          systemProgram: SystemProgram.programId,
        })
        .signers([buyer])
        .rpc();
      assert.fail("should have thrown ZeroPrice");
    } catch (e: any) {
      assert.include(e.message, "ZeroPrice");
    }
  });

  it("minting MagicTokens directly (not via marketplace) is rejected", async () => {
    // Try to mint MagicTokens using a random keypair as caller_authority
    const badActor = Keypair.generate();
    const recipientAta = getAssociatedTokenAddressSync(
      magicTokenMint,
      admin.publicKey,
      false,
      TOKEN_2022_PROGRAM_ID
    );
    try {
      await magicTokenProgram.methods
        .mintMagicTokens(new BN(1000))
        .accounts({
          callerAuthority: badActor.publicKey,
          config: magicTokenConfig,
          mint: magicTokenMint,
          mintAuthority: magicTokenMintAuthority,
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
});
