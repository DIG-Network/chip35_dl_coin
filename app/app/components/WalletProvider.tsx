"use client";

// WalletProvider — React context that exposes wallet connection state.
// Wraps the walletConnect singleton so any component can read address/connected
// and call connect/disconnect without prop drilling.

import React, {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useRef,
  useState,
} from "react";

export interface WalletContextValue {
  address: string | undefined;
  connected: boolean;
  /** Call to begin a pairing flow. Returns { uri, approvalPromise }. */
  startConnect: () => Promise<{ uri: string; approvalPromise: Promise<string | undefined> }>;
  /** Disconnect and clear state. */
  disconnect: () => Promise<void>;
  /** Manually set address after approval resolves (called by WalletConnector). */
  setAddress: (addr: string | undefined) => void;
}

const WalletContext = createContext<WalletContextValue | null>(null);

export function useWallet(): WalletContextValue {
  const ctx = useContext(WalletContext);
  if (!ctx) throw new Error("useWallet must be used inside <WalletProvider>");
  return ctx;
}

export default function WalletProvider({ children }: { children: React.ReactNode }) {
  const [address, setAddressState] = useState<string | undefined>(undefined);
  const initedRef = useRef(false);

  // On mount, initialise walletConnect and restore any existing session.
  useEffect(() => {
    if (typeof window === "undefined") return;
    if (initedRef.current) return;
    initedRef.current = true;

    (async () => {
      const wc = await import("../lib/walletConnect");
      await wc.init();
      const addr = wc.getAddress();
      if (addr) setAddressState(addr);
    })();
  }, []);

  const startConnect = useCallback(async () => {
    const wc = await import("../lib/walletConnect");
    await wc.init();
    return wc.connect();
  }, []);

  const disconnect = useCallback(async () => {
    const wc = await import("../lib/walletConnect");
    await wc.disconnect();
    setAddressState(undefined);
  }, []);

  const setAddress = useCallback((addr: string | undefined) => {
    setAddressState(addr);
  }, []);

  return (
    <WalletContext.Provider
      value={{ address, connected: !!address, startConnect, disconnect, setAddress }}
    >
      {children}
    </WalletContext.Provider>
  );
}
