"use client";
// page.tsx — CHIP-0035 DataLayer Store Dashboard
// Uses dynamic({ssr:false}) so wasm is initialized before the component renders.

import dynamic from "next/dynamic";

const Dashboard = dynamic(
  async () => {
    // Boot the wasm module before the dashboard component renders.
    const { getWasm } = await import("./lib/wasm");
    await getWasm();

    // Import UI components here (inside the factory) so they are only
    // evaluated client-side after wasm is ready.
    const { default: WalletConnector } = await import("./components/WalletConnector");
    const { default: MintForm } = await import("./components/MintForm");
    const { default: StoreList } = await import("./components/StoreList");

    // We need useState for the refresh signal — use a wrapper component
    // defined inside the factory (it is client-only by construction here).
    const { useState } = await import("react");

    function DashboardInner() {
      const [refreshSignal, setRefreshSignal] = useState(0);
      const triggerRefresh = () => setRefreshSignal((n) => n + 1);

      // Read connected state via the walletConnect singleton (no React context
      // needed here — we use a local state that WalletConnector updates via
      // WalletProvider context, which is mounted in layout.tsx).
      // We just render both sections; MintForm disables itself when not connected.
      return (
        <main style={pageStyles.main}>
          {/* Header */}
          <header style={pageStyles.header}>
            <div style={pageStyles.headerInner}>
              <div>
                <h1 style={pageStyles.title}>
                  <span style={pageStyles.brand}>DIG Network</span> · CHIP-0035 Store Demo
                </h1>
                <p style={pageStyles.subtitle}>
                  Mint a store, advance it to a new capsule, and melt it — on Chia mainnet, via the DIG
                  Browser or Sage Wallet
                </p>
              </div>
              <WalletConnector />
            </div>
          </header>

          {/* Content */}
          <div style={pageStyles.content}>
            <MintForm onMinted={triggerRefresh} />
            <StoreList refreshSignal={refreshSignal} />
          </div>

          {/* Footer */}
          <footer style={pageStyles.footer}>
            <p>
              A DIG Network demo &mdash; powered by{" "}
              <a
                href="https://www.npmjs.com/package/@dignetwork/chip35-dl-coin-wasm"
                target="_blank"
                rel="noopener noreferrer"
              >
                @dignetwork/chip35-dl-coin-wasm
              </a>{" "}
              +{" "}
              <a
                href="https://www.walletconnect.com/"
                target="_blank"
                rel="noopener noreferrer"
              >
                WalletConnect
              </a>
            </p>
          </footer>
        </main>
      );
    }

    return DashboardInner;
  },
  {
    ssr: false,
    loading: () => (
      <main
        style={{
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          minHeight: "100vh",
          flexDirection: "column",
          gap: 16,
          color: "var(--dig-ink-3)",
        }}
      >
        <div
          style={{
            width: 36,
            height: 36,
            border: "3px solid var(--dig-border)",
            borderTopColor: "var(--dig-violet)",
            borderRadius: "50%",
            animation: "spin 0.8s linear infinite",
          }}
        />
        <p style={{ margin: 0, fontSize: "0.95rem" }}>Loading WASM module…</p>
      </main>
    ),
  }
);

export default function Home() {
  return <Dashboard />;
}

const pageStyles: Record<string, React.CSSProperties> = {
  main: {
    minHeight: "100vh",
    display: "flex",
    flexDirection: "column",
    background: "var(--dig-bg)",
  },
  header: {
    background: "var(--dig-surface)",
    borderBottom: "1px solid var(--dig-border)",
    padding: "0 24px",
    position: "sticky",
    top: 0,
    zIndex: 100,
  },
  headerInner: {
    maxWidth: 860,
    margin: "0 auto",
    display: "flex",
    alignItems: "center",
    justifyContent: "space-between",
    padding: "16px 0",
    gap: 16,
    flexWrap: "wrap",
  },
  title: {
    margin: 0,
    fontSize: "1.25rem",
    fontWeight: 700,
    color: "var(--dig-ink)",
    letterSpacing: "-0.01em",
  },
  brand: {
    background: "var(--dig-grad)",
    WebkitBackgroundClip: "text",
    backgroundClip: "text",
    color: "transparent",
  },
  subtitle: {
    margin: "2px 0 0",
    fontSize: "0.82rem",
    color: "var(--dig-ink-3)",
  },
  content: {
    maxWidth: 860,
    margin: "0 auto",
    width: "100%",
    padding: "32px 24px",
    display: "flex",
    flexDirection: "column",
    gap: 28,
    flex: 1,
  },
  footer: {
    textAlign: "center",
    padding: "20px 24px",
    fontSize: "0.8rem",
    color: "var(--dig-ink-4)",
    borderTop: "1px solid var(--dig-border)",
    marginTop: "auto",
  },
};
