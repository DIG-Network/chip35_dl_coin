"use client";

// StoreList — render registry entries with liveness checks, update, and delete.

import { useCallback, useEffect, useState } from "react";
import toast from "react-hot-toast";
import UpdateForm from "./UpdateForm";
import { displayStatus, type RegistryEntry } from "../lib/registry";

interface StoreListProps {
  refreshSignal: number; // increment to trigger re-load
}

interface LivenessState {
  loading: boolean;
  spent: boolean | null;
  spentHeight: number | null;
  confirmedHeight: number | null;
}

export default function StoreList({ refreshSignal }: StoreListProps) {
  const [stores, setStores] = useState<RegistryEntry[]>([]);
  const [liveness, setLiveness] = useState<Record<string, LivenessState>>({});
  const [editingId, setEditingId] = useState<string | null>(null);
  const [deletingId, setDeletingId] = useState<string | null>(null);
  const [deletePhase, setDeletePhase] = useState<string | null>(null);
  // Per-store delete fee (mojos); keyed by launcherId
  const [deleteFee, setDeleteFee] = useState<Record<string, string>>({});

  const loadStores = useCallback(() => {
    if (typeof window === "undefined") return;
    // registry is browser-only (localStorage) but safe to static import since
    // it guards internally with `typeof window`. We import dynamically here to
    // satisfy the SSR boundary even though the component is client-only.
    import("../lib/registry").then(({ listStores }) => {
      setStores(listStores());
    });
  }, []);

  useEffect(() => {
    loadStores();
  }, [loadStores, refreshSignal]);

  const checkLiveness = async (entry: RegistryEntry) => {
    const id = entry.launcherId;
    setLiveness((prev) => ({
      ...prev,
      [id]: { loading: true, spent: null, spentHeight: null, confirmedHeight: null },
    }));
    try {
      const { getCoinRecordByName } = await import("../lib/coinset");
      const rec = await getCoinRecordByName(entry.currentCoinIdHex);
      if (rec) {
        setLiveness((prev) => ({
          ...prev,
          [id]: {
            loading: false,
            spent: rec.spentHeight > 0,
            spentHeight: rec.spentHeight > 0 ? rec.spentHeight : null,
            confirmedHeight: rec.confirmedHeight,
          },
        }));
      } else {
        setLiveness((prev) => ({
          ...prev,
          [id]: { loading: false, spent: null, spentHeight: null, confirmedHeight: null },
        }));
        toast("Coin not found on chain yet — it may still be propagating.", {
          icon: "ℹ️",
        });
      }
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : String(e);
      setLiveness((prev) => ({
        ...prev,
        [id]: { loading: false, spent: null, spentHeight: null, confirmedHeight: null },
      }));
      toast.error("Liveness check failed: " + msg);
    }
  };

  const handleDelete = async (entry: RegistryEntry) => {
    if (!window.confirm(`Melt (permanently delete) store "${entry.label || entry.launcherId.slice(0, 14) + "…"}"?\n\nThis cannot be undone.`)) return;

    // Parse the delete fee for this store
    const feeStr = deleteFee[entry.launcherId] ?? "1000000";
    let feeMojos: bigint;
    try {
      feeMojos = BigInt(feeStr);
      if (feeMojos < 0n) throw new Error("negative");
    } catch {
      toast.error("Delete fee must be a non-negative integer (mojos).");
      return;
    }

    setDeletingId(entry.launcherId);
    setDeletePhase("Melting store…");
    const toastId = toast.loading("Melting store…");
    try {
      const { del } = await import("../lib/storeOps");
      await del(entry.launcherId, feeMojos, (s) => {
        setDeletePhase(s);
        toast.loading(s, { id: toastId });
      });
      toast.success("Store melted & confirmed.", { id: toastId });
      loadStores();
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : String(e);
      toast.error("Melt failed: " + msg, { id: toastId, duration: 6000 });
    } finally {
      setDeletingId(null);
      setDeletePhase(null);
    }
  };

  const copyToClipboard = async (text: string) => {
    try {
      await navigator.clipboard.writeText(text);
      toast.success("Copied!");
    } catch {
      toast.error("Copy failed.");
    }
  };

  if (stores.length === 0) {
    return (
      <section style={styles.card}>
        <h2 style={styles.cardTitle}>Your Stores</h2>
        <p style={styles.empty}>No stores yet — mint one above.</p>
      </section>
    );
  }

  return (
    <section style={styles.card}>
      <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", marginBottom: 20 }}>
        <h2 style={{ ...styles.cardTitle, margin: 0 }}>Your Stores</h2>
        <span style={styles.count}>{stores.length} store{stores.length !== 1 ? "s" : ""}</span>
      </div>

      <div style={{ display: "flex", flexDirection: "column", gap: 20 }}>
        {stores.map((entry) => {
          const live = liveness[entry.launcherId];
          const status = displayStatus(entry.status);
          const isDeleted = status === "deleted";
          const isPending = status === "pending";
          const isEditing = editingId === entry.launcherId;
          const isDeleting = deletingId === entry.launcherId;

          const shortId = entry.launcherId.slice(0, 14) + "…" + entry.launcherId.slice(-6);
          const shortCoinId = entry.currentCoinIdHex.slice(0, 10) + "…" + entry.currentCoinIdHex.slice(-6);
          const rootHash = entry.dataStoreJson.metadata.rootHash;
          const shortRootHash = rootHash.slice(0, 10) + "…" + rootHash.slice(-6);
          const programHash = entry.dataStoreJson.metadata.programHash;
          const shortProgramHash = programHash
            ? programHash.slice(0, 10) + "…" + programHash.slice(-6)
            : null;

          return (
            <div
              key={entry.launcherId}
              style={{
                ...styles.storeRow,
                opacity: isDeleted ? 0.55 : 1,
                background: isDeleted ? "#f9fafb" : "#fff",
              }}
            >
              {/* Header */}
              <div style={styles.storeHeader}>
                <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                  <span style={styles.storeLabel}>
                    {entry.label || <em style={{ color: "#9ca3af" }}>Unlabelled</em>}
                  </span>
                  <span
                    style={
                      isDeleted
                        ? styles.badgeDeleted
                        : isPending
                        ? styles.badgePending
                        : styles.badgeLive
                    }
                  >
                    {isDeleted ? "deleted" : isPending ? "pending" : "confirmed"}
                  </span>
                </div>
                {!isDeleted && (
                  <div style={styles.actions}>
                    <button
                      style={styles.btnAction}
                      onClick={() => setEditingId(isEditing ? null : entry.launcherId)}
                      disabled={isDeleting}
                    >
                      {isEditing ? "Cancel Edit" : "Update"}
                    </button>
                    <input
                      type="number"
                      min="0"
                      step="1"
                      title="Delete fee (mojos)"
                      placeholder="Fee (mojos)"
                      value={deleteFee[entry.launcherId] ?? "1000000"}
                      onChange={(e) =>
                        setDeleteFee((prev) => ({
                          ...prev,
                          [entry.launcherId]: e.target.value,
                        }))
                      }
                      disabled={isDeleting}
                      style={{
                        width: 120,
                        border: "1px solid #d1d5db",
                        borderRadius: 7,
                        padding: "4px 8px",
                        fontSize: "0.8rem",
                        background: "#fafafa",
                      }}
                    />
                    <button
                      style={{ ...styles.btnAction, color: "#dc2626", borderColor: "#dc2626" }}
                      onClick={() => handleDelete(entry)}
                      disabled={isDeleting}
                    >
                      {isDeleting ? deletePhase ?? "Melting…" : "Delete"}
                    </button>
                  </div>
                )}
              </div>

              {/* Description */}
              {entry.dataStoreJson.metadata.description && (
                <p style={styles.description}>{entry.dataStoreJson.metadata.description}</p>
              )}

              {/* Fields */}
              <div style={styles.fields}>
                <FieldRow
                  label="Launcher ID"
                  value={shortId}
                  fullValue={entry.launcherId}
                  onCopy={() => copyToClipboard(entry.launcherId)}
                />
                <FieldRow
                  label="Current Coin"
                  value={shortCoinId}
                  fullValue={entry.currentCoinIdHex}
                  onCopy={() => copyToClipboard(entry.currentCoinIdHex)}
                  link={`https://spacescan.io/coin/0x${entry.currentCoinIdHex}`}
                />
                <FieldRow
                  label="Root Hash"
                  value={shortRootHash}
                  fullValue={rootHash}
                  onCopy={() => copyToClipboard(rootHash)}
                />
                {programHash && shortProgramHash && (
                  <FieldRow
                    label="Program Hash"
                    value={shortProgramHash}
                    fullValue={programHash}
                    onCopy={() => copyToClipboard(programHash)}
                  />
                )}
              </div>

              {/* Liveness row */}
              <div style={styles.livenessRow}>
                {live?.loading ? (
                  <span style={styles.livenessLoading}>Checking chain…</span>
                ) : live && live.spent !== null ? (
                  <span style={live.spent ? styles.livenessSpent : styles.livenessUnspent}>
                    {live.spent
                      ? `Spent at block ${live.spentHeight}`
                      : `Unspent (confirmed at block ${live.confirmedHeight})`}
                  </span>
                ) : null}
                {!isDeleted && (
                  <button
                    style={styles.btnRefresh}
                    onClick={() => checkLiveness(entry)}
                    disabled={live?.loading}
                  >
                    {live?.loading ? "Checking…" : "Refresh Status"}
                  </button>
                )}
              </div>

              {/* Inline update form */}
              {isEditing && (
                <UpdateForm
                  launcherId={entry.launcherId}
                  currentLabel={entry.label}
                  currentDescription={entry.dataStoreJson.metadata.description ?? ""}
                  currentRootHash={entry.dataStoreJson.metadata.rootHash}
                  onUpdated={() => {
                    setEditingId(null);
                    loadStores();
                  }}
                  onCancel={() => setEditingId(null)}
                />
              )}
            </div>
          );
        })}
      </div>
    </section>
  );
}

function FieldRow({
  label,
  value,
  fullValue,
  onCopy,
  link,
}: {
  label: string;
  value: string;
  fullValue: string;
  onCopy: () => void;
  link?: string;
}) {
  return (
    <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
      <span style={{ fontSize: "0.8rem", color: "#6b7280", minWidth: 100 }}>{label}:</span>
      {link ? (
        <a
          href={link}
          target="_blank"
          rel="noopener noreferrer"
          style={{ fontFamily: "monospace", fontSize: "0.82rem", color: "#2563eb" }}
          title={fullValue}
        >
          {value}
        </a>
      ) : (
        <span
          style={{ fontFamily: "monospace", fontSize: "0.82rem", color: "#374151" }}
          title={fullValue}
        >
          {value}
        </span>
      )}
      <button
        onClick={onCopy}
        title="Copy to clipboard"
        style={{
          background: "none",
          border: "none",
          cursor: "pointer",
          fontSize: "0.8rem",
          color: "#9ca3af",
          padding: "0 2px",
          lineHeight: 1,
        }}
      >
        ⧉
      </button>
    </div>
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
  count: {
    fontSize: "0.82rem",
    color: "#6b7280",
    background: "#f3f4f6",
    borderRadius: 20,
    padding: "3px 10px",
  },
  empty: {
    color: "#9ca3af",
    fontSize: "0.95rem",
    margin: 0,
    textAlign: "center",
    padding: "24px 0",
  },
  storeRow: {
    border: "1px solid #e5e7eb",
    borderRadius: 10,
    padding: "18px 20px",
    display: "flex",
    flexDirection: "column",
    gap: 10,
  },
  storeHeader: {
    display: "flex",
    alignItems: "center",
    justifyContent: "space-between",
    flexWrap: "wrap",
    gap: 10,
  },
  storeLabel: {
    fontSize: "1rem",
    fontWeight: 600,
    color: "#111827",
  },
  badgeLive: {
    fontSize: "0.72rem",
    fontWeight: 700,
    background: "#dcfce7",
    color: "#16a34a",
    borderRadius: 20,
    padding: "2px 9px",
    letterSpacing: "0.04em",
    textTransform: "uppercase",
  },
  badgePending: {
    fontSize: "0.72rem",
    fontWeight: 700,
    background: "#fef3c7",
    color: "#d97706",
    borderRadius: 20,
    padding: "2px 9px",
    letterSpacing: "0.04em",
    textTransform: "uppercase",
  },
  badgeDeleted: {
    fontSize: "0.72rem",
    fontWeight: 700,
    background: "#fee2e2",
    color: "#dc2626",
    borderRadius: 20,
    padding: "2px 9px",
    letterSpacing: "0.04em",
    textTransform: "uppercase",
  },
  description: {
    margin: 0,
    fontSize: "0.85rem",
    color: "#6b7280",
    fontStyle: "italic",
  },
  fields: {
    display: "flex",
    flexDirection: "column",
    gap: 6,
    padding: "8px 0",
  },
  actions: {
    display: "flex",
    gap: 8,
  },
  btnAction: {
    background: "transparent",
    border: "1px solid #d1d5db",
    borderRadius: 7,
    padding: "5px 12px",
    fontSize: "0.82rem",
    cursor: "pointer",
    color: "#374151",
  },
  livenessRow: {
    display: "flex",
    alignItems: "center",
    gap: 12,
    flexWrap: "wrap",
    marginTop: 2,
  },
  livenessLoading: {
    fontSize: "0.8rem",
    color: "#9ca3af",
    fontStyle: "italic",
  },
  livenessUnspent: {
    fontSize: "0.8rem",
    color: "#16a34a",
    fontWeight: 600,
  },
  livenessSpent: {
    fontSize: "0.8rem",
    color: "#dc2626",
    fontWeight: 600,
  },
  btnRefresh: {
    background: "transparent",
    border: "1px solid #d1d5db",
    borderRadius: 6,
    padding: "4px 12px",
    fontSize: "0.78rem",
    cursor: "pointer",
    color: "#6b7280",
  },
};
