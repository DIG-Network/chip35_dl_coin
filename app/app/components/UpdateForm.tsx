"use client";

// UpdateForm — inline form to update metadata of an existing store.
// Rendered inside StoreList when the user clicks "Update".

import { useState } from "react";
import toast from "react-hot-toast";

interface UpdateFormProps {
  launcherId: string;
  currentLabel: string;
  currentDescription: string;
  currentRootHash: string; // 0x-prefixed hex from registry
  currentProgramHash?: string; // 0x-prefixed hex from registry (may be empty/undefined)
  onUpdated: () => void;
  onCancel: () => void;
}

function random32Hex(): string {
  const bytes = new Uint8Array(32);
  crypto.getRandomValues(bytes);
  return Array.from(bytes)
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
}

export default function UpdateForm({
  launcherId,
  currentLabel,
  currentDescription,
  currentRootHash,
  currentProgramHash,
  onUpdated,
  onCancel,
}: UpdateFormProps) {
  // Strip 0x for display. Pre-fill the program hash with the store's current
  // value so editing other fields doesn't silently drop it (blank => omitted).
  const initHash = currentRootHash.replace(/^0x/i, "");
  const initProgramHash = (currentProgramHash ?? "").replace(/^0x/i, "");
  const [newRootHash, setNewRootHash] = useState(initHash);
  const [newLabel, setNewLabel] = useState(currentLabel);
  const [newDescription, setNewDescription] = useState(currentDescription);
  const [newProgramHash, setNewProgramHash] = useState(initProgramHash);
  const [fee, setFee] = useState("1000000");
  const [submitting, setSubmitting] = useState(false);
  const [phase, setPhase] = useState<string | null>(null);

  const handleRandomHash = () => setNewRootHash(random32Hex());
  const handleRandomProgramHash = () => setNewProgramHash(random32Hex());

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    const cleanHash = newRootHash.replace(/^0x/i, "");
    if (!/^[0-9a-fA-F]{64}$/.test(cleanHash)) {
      toast.error("Root hash must be 64 hex characters.");
      return;
    }
    // Validate new program hash (optional)
    let cleanNewProgramHash: string | undefined;
    if (newProgramHash.trim()) {
      cleanNewProgramHash = newProgramHash.trim().replace(/^0x/i, "");
      if (!/^[0-9a-fA-F]{64}$/.test(cleanNewProgramHash)) {
        toast.error("New program hash must be 64 hex characters (32 bytes) if provided.");
        return;
      }
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
    setPhase("Updating store metadata…");
    const toastId = toast.loading("Updating store metadata…");
    try {
      const { updateMetadata } = await import("../lib/storeOps");
      await updateMetadata(
        launcherId,
        {
          newRootHashHex: cleanHash,
          newLabel: newLabel.trim() || undefined,
          newDescription: newDescription.trim() || undefined,
          feeMojos,
          newProgramHashHex: cleanNewProgramHash,
        },
        (s) => {
          setPhase(s);
          toast.loading(s, { id: toastId });
        }
      );
      toast.success("Store updated & confirmed!", { id: toastId });
      onUpdated();
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : String(e);
      toast.error("Update failed: " + msg, { id: toastId, duration: 6000 });
    } finally {
      setSubmitting(false);
      setPhase(null);
    }
  };

  return (
    <form onSubmit={handleSubmit} style={styles.form}>
      <p style={styles.formNote}>
        Saving advances this store to a new <strong>capsule</strong> (a fresh
        <code style={{ fontFamily: "var(--dig-font-mono)" }}> storeId:rootHash</code> generation).
      </p>
      <label style={styles.label}>
        New Label <span style={styles.optional}>(optional)</span>
        <input
          style={styles.input}
          type="text"
          value={newLabel}
          onChange={(e) => setNewLabel(e.target.value)}
          disabled={submitting}
        />
      </label>

      <label style={styles.label}>
        New Description <span style={styles.optional}>(optional)</span>
        <input
          style={styles.input}
          type="text"
          value={newDescription}
          onChange={(e) => setNewDescription(e.target.value)}
          disabled={submitting}
        />
      </label>

      <label style={styles.label}>
        New Root Hash
        <div style={{ display: "flex", gap: 8 }}>
          <input
            style={{ ...styles.input, fontFamily: "var(--dig-font-mono)", fontSize: "0.78rem", flex: 1 }}
            type="text"
            value={newRootHash}
            onChange={(e) => setNewRootHash(e.target.value)}
            spellCheck={false}
            disabled={submitting}
          />
          <button
            type="button"
            style={styles.btnSecondary}
            onClick={handleRandomHash}
            disabled={submitting}
          >
            Random
          </button>
        </div>
      </label>

      <label style={styles.label}>
        New Program Hash <span style={styles.optional}>(32-byte hex, optional)</span>
        <div style={{ display: "flex", gap: 8 }}>
          <input
            style={{ ...styles.input, fontFamily: "var(--dig-font-mono)", fontSize: "0.78rem", flex: 1 }}
            type="text"
            value={newProgramHash}
            onChange={(e) => setNewProgramHash(e.target.value)}
            placeholder={"(leave blank to omit)"}
            spellCheck={false}
            disabled={submitting}
          />
          <button
            type="button"
            style={styles.btnSecondary}
            onClick={handleRandomProgramHash}
            disabled={submitting}
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

      <div style={{ display: "flex", gap: 10 }}>
        <button
          type="submit"
          style={{
            ...styles.btnPrimary,
            opacity: submitting ? 0.5 : 1,
            cursor: submitting ? "not-allowed" : "pointer",
          }}
          disabled={submitting}
        >
          {submitting ? phase ?? "Updating…" : "Save Changes"}
        </button>
        <button
          type="button"
          style={styles.btnSecondary}
          onClick={onCancel}
          disabled={submitting}
        >
          Cancel
        </button>
      </div>

      {submitting && phase && (
        <p style={styles.phase} aria-live="polite">
          {phase}
        </p>
      )}
    </form>
  );
}

const styles: Record<string, React.CSSProperties> = {
  form: {
    display: "flex",
    flexDirection: "column",
    gap: 14,
    padding: "16px 0 4px",
    borderTop: "1px solid var(--dig-border)",
    marginTop: 12,
  },
  formNote: {
    margin: 0,
    fontSize: "0.82rem",
    color: "var(--dig-ink-3)",
  },
  label: {
    display: "flex",
    flexDirection: "column",
    gap: 5,
    fontSize: "0.88rem",
    fontWeight: 600,
    color: "var(--dig-ink-2)",
  },
  optional: {
    fontWeight: 400,
    color: "var(--dig-ink-4)",
    fontSize: "0.78rem",
  },
  input: {
    border: "1px solid var(--dig-border-input)",
    borderRadius: 7,
    padding: "8px 11px",
    fontSize: "0.88rem",
    outline: "none",
    background: "var(--dig-well)",
  },
  btnPrimary: {
    background: "var(--dig-grad)",
    color: "var(--dig-surface)",
    border: "none",
    borderRadius: 8,
    padding: "9px 20px",
    fontSize: "0.9rem",
    fontWeight: 600,
    cursor: "pointer",
  },
  btnSecondary: {
    background: "transparent",
    color: "var(--dig-accent)",
    border: "1px solid var(--dig-accent)",
    borderRadius: 7,
    padding: "8px 14px",
    fontSize: "0.85rem",
    cursor: "pointer",
    whiteSpace: "nowrap",
  },
  phase: {
    margin: 0,
    fontSize: "0.82rem",
    color: "var(--dig-accent)",
    fontStyle: "italic",
  },
};
