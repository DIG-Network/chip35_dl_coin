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
  onUpdated: () => void;
  onCancel: () => void;
}

function randomRootHash(): string {
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
  onUpdated,
  onCancel,
}: UpdateFormProps) {
  // Strip 0x for display
  const initHash = currentRootHash.replace(/^0x/i, "");
  const [newRootHash, setNewRootHash] = useState(initHash);
  const [newLabel, setNewLabel] = useState(currentLabel);
  const [newDescription, setNewDescription] = useState(currentDescription);
  const [submitting, setSubmitting] = useState(false);

  const handleRandomHash = () => setNewRootHash(randomRootHash());

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    const cleanHash = newRootHash.replace(/^0x/i, "");
    if (!/^[0-9a-fA-F]{64}$/.test(cleanHash)) {
      toast.error("Root hash must be 64 hex characters.");
      return;
    }
    setSubmitting(true);
    const toastId = toast.loading("Updating store metadata…");
    try {
      const { updateMetadata } = await import("../lib/storeOps");
      await updateMetadata(launcherId, {
        newRootHashHex: cleanHash,
        newLabel: newLabel.trim() || undefined,
        newDescription: newDescription.trim() || undefined,
      });
      toast.success("Store updated!", { id: toastId });
      onUpdated();
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : String(e);
      toast.error("Update failed: " + msg, { id: toastId, duration: 6000 });
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <form onSubmit={handleSubmit} style={styles.form}>
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
            style={{ ...styles.input, fontFamily: "monospace", fontSize: "0.78rem", flex: 1 }}
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
          {submitting ? "Updating…" : "Save Changes"}
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
    </form>
  );
}

const styles: Record<string, React.CSSProperties> = {
  form: {
    display: "flex",
    flexDirection: "column",
    gap: 14,
    padding: "16px 0 4px",
    borderTop: "1px solid #f3f4f6",
    marginTop: 12,
  },
  label: {
    display: "flex",
    flexDirection: "column",
    gap: 5,
    fontSize: "0.88rem",
    fontWeight: 600,
    color: "#374151",
  },
  optional: {
    fontWeight: 400,
    color: "#9ca3af",
    fontSize: "0.78rem",
  },
  input: {
    border: "1px solid #d1d5db",
    borderRadius: 7,
    padding: "8px 11px",
    fontSize: "0.88rem",
    outline: "none",
    background: "#fafafa",
  },
  btnPrimary: {
    background: "#2563eb",
    color: "#fff",
    border: "none",
    borderRadius: 8,
    padding: "9px 20px",
    fontSize: "0.9rem",
    fontWeight: 600,
    cursor: "pointer",
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
};
