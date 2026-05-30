"use client";

// MintForm — mint a new CHIP-0035 DataLayer store.
// Calls storeOps.mint() which handles wasm + WalletConnect signing internally.

import { useState } from "react";
import toast from "react-hot-toast";
import { useWallet } from "./WalletProvider";

interface MintFormProps {
  onMinted: () => void; // trigger StoreList refresh
}

function randomRootHash(): string {
  const bytes = new Uint8Array(32);
  crypto.getRandomValues(bytes);
  return Array.from(bytes)
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
}

const ZERO_HASH = "0".repeat(64);

export default function MintForm({ onMinted }: MintFormProps) {
  const { connected } = useWallet();
  const [label, setLabel] = useState("");
  const [description, setDescription] = useState("");
  const [rootHash, setRootHash] = useState(ZERO_HASH);
  const [fee, setFee] = useState("0");
  const [submitting, setSubmitting] = useState(false);
  const [phase, setPhase] = useState<string | null>(null);

  const handleRandomHash = () => setRootHash(randomRootHash());

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!connected) {
      toast.error("Connect your wallet first.");
      return;
    }
    // Validate hex
    const cleanHash = rootHash.replace(/^0x/i, "");
    if (!/^[0-9a-fA-F]{64}$/.test(cleanHash)) {
      toast.error("Root hash must be 64 hex characters (32 bytes).");
      return;
    }
    let feeMojos: bigint;
    try {
      feeMojos = BigInt(fee);
      if (feeMojos < 0n) throw new Error("negative");
    } catch {
      toast.error("Fee must be a non-negative integer (mojos).");
      return;
    }

    setSubmitting(true);
    setPhase("Minting store…");
    const toastId = toast.loading("Minting store…");
    try {
      const { mint } = await import("../lib/storeOps");
      const result = await mint(
        {
          label: label.trim() || undefined,
          description: description.trim() || undefined,
          rootHashHex: cleanHash,
          feeMojos,
        },
        (s) => {
          setPhase(s);
          toast.loading(s, { id: toastId });
        }
      );
      toast.success(
        `Store minted & confirmed! Launcher ID: ${result.launcherIdHex.slice(0, 14)}…`,
        { id: toastId, duration: 5000 }
      );
      setLabel("");
      setDescription("");
      setRootHash(ZERO_HASH);
      setFee("0");
      // Surface the pending entry immediately, and again after confirm.
      onMinted();
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : String(e);
      toast.error("Mint failed: " + msg, { id: toastId, duration: 6000 });
      // The store may have been left pending — refresh so it shows up.
      onMinted();
    } finally {
      setSubmitting(false);
      setPhase(null);
    }
  };

  return (
    <section style={styles.card}>
      <h2 style={styles.cardTitle}>Mint New Store</h2>
      <form onSubmit={handleSubmit} style={styles.form}>
        <label style={styles.label}>
          Label <span style={styles.optional}>(optional)</span>
          <input
            style={styles.input}
            type="text"
            value={label}
            onChange={(e) => setLabel(e.target.value)}
            placeholder="My DataLayer Store"
            disabled={submitting}
          />
        </label>

        <label style={styles.label}>
          Description <span style={styles.optional}>(optional)</span>
          <input
            style={styles.input}
            type="text"
            value={description}
            onChange={(e) => setDescription(e.target.value)}
            placeholder="Short description…"
            disabled={submitting}
          />
        </label>

        <label style={styles.label}>
          Root Hash <span style={styles.optional}>(64 hex chars)</span>
          <div style={{ display: "flex", gap: 8 }}>
            <input
              style={{ ...styles.input, fontFamily: "monospace", fontSize: "0.8rem", flex: 1 }}
              type="text"
              value={rootHash}
              onChange={(e) => setRootHash(e.target.value)}
              placeholder={ZERO_HASH}
              disabled={submitting}
              spellCheck={false}
            />
            <button
              type="button"
              style={styles.btnSecondary}
              onClick={handleRandomHash}
              disabled={submitting}
              title="Fill with random 32 bytes"
            >
              Random
            </button>
          </div>
        </label>

        <label style={styles.label}>
          Fee <span style={styles.optional}>(mojos)</span>
          <input
            style={{ ...styles.input, width: 160 }}
            type="number"
            min="0"
            step="1"
            value={fee}
            onChange={(e) => setFee(e.target.value)}
            disabled={submitting}
          />
        </label>

        <button
          type="submit"
          style={{
            ...styles.btnPrimary,
            opacity: !connected || submitting ? 0.5 : 1,
            cursor: !connected || submitting ? "not-allowed" : "pointer",
          }}
          disabled={!connected || submitting}
        >
          {submitting ? phase ?? "Minting…" : "Mint Store"}
        </button>

        {submitting && phase && (
          <p style={styles.phase} aria-live="polite">
            {phase}
          </p>
        )}

        {!connected && (
          <p style={styles.notice}>Connect your wallet to enable minting.</p>
        )}
      </form>
    </section>
  );
}

const styles: Record<string, React.CSSProperties> = {
  card: {
    background: "#fff",
    border: "1px solid #e5e7eb",
    borderRadius: 12,
    padding: "24px 28px",
    boxShadow: "0 1px 6px rgba(0,0,0,0.06)",
  },
  cardTitle: {
    margin: "0 0 20px",
    fontSize: "1.15rem",
    fontWeight: 700,
    color: "#111827",
  },
  form: {
    display: "flex",
    flexDirection: "column",
    gap: 16,
  },
  label: {
    display: "flex",
    flexDirection: "column",
    gap: 6,
    fontSize: "0.9rem",
    fontWeight: 600,
    color: "#374151",
  },
  optional: {
    fontWeight: 400,
    color: "#9ca3af",
    fontSize: "0.8rem",
  },
  input: {
    border: "1px solid #d1d5db",
    borderRadius: 7,
    padding: "9px 12px",
    fontSize: "0.9rem",
    outline: "none",
    transition: "border-color 0.15s",
    background: "#fafafa",
  },
  btnPrimary: {
    alignSelf: "flex-start",
    background: "#2563eb",
    color: "#fff",
    border: "none",
    borderRadius: 8,
    padding: "10px 24px",
    fontSize: "0.95rem",
    fontWeight: 600,
    cursor: "pointer",
    transition: "opacity 0.15s",
  },
  btnSecondary: {
    background: "transparent",
    color: "#2563eb",
    border: "1px solid #2563eb",
    borderRadius: 7,
    padding: "8px 14px",
    fontSize: "0.85rem",
    cursor: "pointer",
    whiteSpace: "nowrap",
  },
  notice: {
    margin: 0,
    fontSize: "0.82rem",
    color: "#f59e0b",
    fontStyle: "italic",
  },
  phase: {
    margin: 0,
    fontSize: "0.82rem",
    color: "#2563eb",
    fontStyle: "italic",
  },
};
