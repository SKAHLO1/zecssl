"use client";

// NEAR wallet connection (NEAR Wallet Selector) for the steps a user signs themselves:
// locking vault tokens for a shielded ZEC payout. Loaded on first use, in the browser only.
import "@near-wallet-selector/modal-ui/styles.css";
import type { WalletSelector } from "@near-wallet-selector/core";
import type { WalletSelectorModal } from "@near-wallet-selector/modal-ui";
import { useCallback, useEffect, useState } from "react";
import type { PayoutQuote } from "./types";

type Loaded = { selector: WalletSelector; modal: WalletSelectorModal };
let loading: Promise<Loaded> | null = null;

function load(network: "testnet" | "mainnet"): Promise<Loaded> {
  loading ??= (async () => {
    const [
      { setupWalletSelector },
      { setupModal },
      { setupMeteorWallet },
      { setupHereWallet },
      { setupIntearWallet },
      { setupSender },
      { setupNightly },
      { setupLedger },
      { setupMyNearWallet },
    ] = await Promise.all([
      import("@near-wallet-selector/core"),
      import("@near-wallet-selector/modal-ui"),
      import("@near-wallet-selector/meteor-wallet"),
      import("@near-wallet-selector/here-wallet"),
      import("@near-wallet-selector/intear-wallet"),
      import("@near-wallet-selector/sender"),
      import("@near-wallet-selector/nightly"),
      import("@near-wallet-selector/ledger"),
      import("@near-wallet-selector/my-near-wallet"),
    ]);
    // Browser, mobile, extension and hardware wallets, all working on testnet. MyNearWallet is
    // being sunset (deprecated 31 October 2026), so it is marked deprecated and listed last.
    const selector = await setupWalletSelector({
      network,
      modules: [
        setupMeteorWallet(),
        setupHereWallet(),
        setupIntearWallet(),
        setupSender(),
        setupNightly(),
        setupLedger(),
        setupMyNearWallet({ deprecated: true }),
      ],
    });
    const modal = setupModal(selector, { description: "Connect the NEAR account that holds the tokens you want to swap for shielded ZEC." });
    return { selector, modal };
  })();
  return loading;
}

/** 100 TGas covers ft_transfer_call plus the vault's ft_on_transfer and the resolve. */
const TRANSFER_GAS = BigInt(100) * BigInt(10) ** BigInt(12);

export function useNearWallet(network: "testnet" | "mainnet" = "testnet") {
  const [accountId, setAccountId] = useState<string | null>(null);
  const [ready, setReady] = useState(false);

  useEffect(() => {
    // Keep the subscription object itself: its unsubscribe method needs it as `this`, and
    // calling a detached copy throws during cleanup, which crashed page navigation.
    let subscription: { unsubscribe: () => void } | undefined;
    let cancelled = false;
    load(network)
      .then(({ selector }) => {
        if (cancelled) return;
        const pick = () => selector.store.getState().accounts.find((a) => a.active)?.accountId ?? null;
        setAccountId(pick());
        setReady(true);
        subscription = selector.store.observable.subscribe(() => setAccountId(pick()));
      })
      .catch(() => setReady(true));
    return () => {
      cancelled = true;
      subscription?.unsubscribe();
    };
  }, [network]);

  const connect = useCallback(async () => (await load(network)).modal.show(), [network]);

  const disconnect = useCallback(async () => {
    const { selector } = await load(network);
    if (selector.isSignedIn()) await (await selector.wallet()).signOut();
  }, [network]);

  /** Sends vault tokens to the vault with an ft_transfer_call from the connected account.
   * Browser wallets navigate away and come back; injected ones resolve with the outcome. */
  const transferCall = useCallback(
    async (token: string, args: { receiver_id: string; amount: string; msg: string }) => {
      const { selector } = await load(network);
      const { actionCreators } = await import("@near-wallet-selector/core");
      const wallet = await selector.wallet();
      return wallet.signAndSendTransaction({
        receiverId: token,
        actions: [actionCreators.functionCall("ft_transfer_call", args, TRANSFER_GAS, BigInt(1))],
      });
    },
    [network],
  );

  /** Locks tokens for a quoted ZEC payout. */
  const lock = useCallback((quote: PayoutQuote) => transferCall(quote.token, quote.transfer), [transferCall]);

  /** Challenges a direction-2 settlement claim, posting the challenge bond. */
  const challenge = useCallback(
    (token: string, vault: string, orderId: string, bondRaw: string) =>
      transferCall(token, { receiver_id: vault, amount: bondRaw, msg: JSON.stringify({ action: "challenge", order_id: orderId }) }),
    [transferCall],
  );

  return { accountId, ready, connect, disconnect, lock, challenge };
}
