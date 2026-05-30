"use client";
// page.tsx — minimal placeholder that proves the wasm import pattern works.
// Full UI (connect wallet / list / mint / update / delete) is the next dispatch.
// "use client" is required because dynamic({ssr: false}) is not allowed in
// React Server Components (Next.js 15 App Router rule).

import dynamic from "next/dynamic";

// The wasm package must only be loaded client-side. We use Next.js's `dynamic`
// with `ssr: false` and pass an async factory so `getWasm()` is awaited before
// the component renders. This is the canonical wasm-in-Next pattern.
const Chip35Page = dynamic(
  async () => {
    // Lazy-load the wasm module. This both verifies the import resolves
    // correctly at runtime and initialises the wasm init() hook.
    const { getWasm } = await import("./lib/wasm");
    await getWasm();

    // Return the actual React component as the default export of the factory.
    function Page() {
      return (
        <main
          style={{
            display: "flex",
            flexDirection: "column",
            alignItems: "center",
            justifyContent: "center",
            minHeight: "100vh",
            padding: "2rem",
            textAlign: "center",
          }}
        >
          <h1 style={{ fontSize: "2rem", marginBottom: "1rem" }}>
            CHIP-0035 DataLayer Store Demo
          </h1>
          <p style={{ color: "#555", maxWidth: 480 }}>
            Wasm module loaded successfully. Full UI (connect wallet / list /
            mint / update / delete stores) is coming in the next step.
          </p>
          <p style={{ marginTop: "1.5rem", color: "#888", fontSize: "0.85rem" }}>
            Connect your Sage Wallet via WalletConnect to get started.
          </p>
        </main>
      );
    }

    return Page;
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
        }}
      >
        <p style={{ color: "#999" }}>Loading WASM module…</p>
      </main>
    ),
  }
);

export default function Home() {
  return <Chip35Page />;
}
