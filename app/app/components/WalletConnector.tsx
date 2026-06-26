"use client";

// WalletConnector — "Connect Wallet" button + QR modal + session status bar.
// Uses chip35's walletConnect.ts (returns {uri, approvalPromise} from connect()).

import { useState } from "react";
import { QRCodeSVG } from "qrcode.react";
import toast from "react-hot-toast";
import { useWallet } from "./WalletProvider";
import { INJECTED_URI } from "../lib/walletConnect";

function truncAddr(addr: string): string {
  if (addr.length <= 16) return addr;
  return addr.slice(0, 10) + "…" + addr.slice(-6);
}

export default function WalletConnector() {
  const { address, connected, injectedAvailable, startConnect, disconnect, setAddress } =
    useWallet();
  const [modalOpen, setModalOpen] = useState(false);
  const [qrUri, setQrUri] = useState<string | undefined>();
  const [awaitingApproval, setAwaitingApproval] = useState(false);
  const [copied, setCopied] = useState(false);

  const handleConnect = async () => {
    setQrUri(undefined);
    setAwaitingApproval(false);
    try {
      const { uri, approvalPromise } = await startConnect();
      // DIG Browser injected wallet: no QR/relay — approval happens in the
      // native wallet UI. Skip the QR modal and just wait for approval.
      const injected = uri === INJECTED_URI;
      if (!injected) {
        setModalOpen(true);
        setQrUri(uri);
      }
      setAwaitingApproval(true);
      const addr = await approvalPromise;
      if (addr) {
        setAddress(addr);
        toast.success("Wallet connected: " + truncAddr(addr));
      } else {
        toast.error("Wallet connection was rejected or timed out.");
      }
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : String(e);
      toast.error("Connection failed: " + msg);
    } finally {
      setModalOpen(false);
      setAwaitingApproval(false);
    }
  };

  const handleDisconnect = async () => {
    await disconnect();
    toast.success("Wallet disconnected.");
  };

  const handleCopy = async () => {
    if (!qrUri) return;
    try {
      await navigator.clipboard.writeText(qrUri);
      setCopied(true);
      toast.success("Link copied!");
      setTimeout(() => setCopied(false), 1500);
    } catch {
      toast.error("Could not copy to clipboard.");
    }
  };

  const handleModalClose = () => {
    setModalOpen(false);
    setQrUri(undefined);
    setAwaitingApproval(false);
  };

  return (
    <>
      {/* Button row */}
      {connected && address ? (
        <div style={{ display: "flex", alignItems: "center", gap: 12 }}>
          <span style={styles.addressBadge} title={address}>
            {truncAddr(address)}
          </span>
          <button style={styles.btnSecondary} onClick={handleDisconnect}>
            Disconnect
          </button>
        </div>
      ) : (
        <button style={styles.btnPrimary} onClick={handleConnect}>
          {injectedAvailable ? "Connect DIG Wallet" : "Connect Wallet"}
        </button>
      )}

      {/* QR Modal */}
      {modalOpen && (
        <div style={styles.overlay} onClick={handleModalClose}>
          <div
            style={styles.modal}
            onClick={(e) => e.stopPropagation()}
            role="dialog"
            aria-modal="true"
            aria-label="Connect Sage Wallet"
          >
            <div style={styles.modalHeader}>
              <h2 style={{ margin: 0, fontSize: "1.1rem" }}>Connect Sage Wallet</h2>
              <button style={styles.closeBtn} onClick={handleModalClose} aria-label="Close">
                ✕
              </button>
            </div>

            <div style={styles.modalBody}>
              {qrUri ? (
                <>
                  <div style={styles.qrWrapper}>
                    <QRCodeSVG value={qrUri} size={240} />
                  </div>
                  <p style={styles.hint}>
                    Scan this QR code in Sage Wallet, or copy the link and paste it into
                    WalletConnect.
                  </p>
                  <button style={styles.btnSecondary} onClick={handleCopy}>
                    {copied ? "Copied!" : "Copy Link"}
                  </button>
                  {awaitingApproval && (
                    <p style={styles.waiting}>Waiting for wallet approval…</p>
                  )}
                </>
              ) : (
                <div style={styles.spinnerWrap}>
                  <div style={styles.spinner} />
                  <p style={styles.hint}>Initialising WalletConnect…</p>
                </div>
              )}
            </div>
          </div>
        </div>
      )}
    </>
  );
}

const styles: Record<string, React.CSSProperties> = {
  btnPrimary: {
    background: "#2563eb",
    color: "#fff",
    border: "none",
    borderRadius: 8,
    padding: "10px 22px",
    fontSize: "0.95rem",
    fontWeight: 600,
    cursor: "pointer",
  },
  btnSecondary: {
    background: "transparent",
    color: "#2563eb",
    border: "1px solid #2563eb",
    borderRadius: 8,
    padding: "8px 16px",
    fontSize: "0.9rem",
    cursor: "pointer",
  },
  addressBadge: {
    fontFamily: "monospace",
    fontSize: "0.9rem",
    background: "#f0f4ff",
    border: "1px solid #c7d7ff",
    borderRadius: 6,
    padding: "6px 12px",
    color: "#1e3a8a",
  },
  overlay: {
    position: "fixed",
    inset: 0,
    background: "rgba(0,0,0,0.55)",
    display: "flex",
    alignItems: "center",
    justifyContent: "center",
    zIndex: 1000,
  },
  modal: {
    background: "#fff",
    borderRadius: 12,
    boxShadow: "0 8px 40px rgba(0,0,0,0.18)",
    width: "min(95vw, 380px)",
    overflow: "hidden",
  },
  modalHeader: {
    display: "flex",
    alignItems: "center",
    justifyContent: "space-between",
    padding: "16px 20px",
    borderBottom: "1px solid #e5e7eb",
  },
  modalBody: {
    padding: "24px 20px",
    display: "flex",
    flexDirection: "column",
    alignItems: "center",
    gap: 16,
  },
  qrWrapper: {
    background: "#fff",
    padding: 12,
    border: "1px solid #e5e7eb",
    borderRadius: 8,
  },
  hint: {
    margin: 0,
    fontSize: "0.85rem",
    color: "#6b7280",
    textAlign: "center",
    maxWidth: 300,
  },
  waiting: {
    margin: 0,
    fontSize: "0.82rem",
    color: "#9ca3af",
    textAlign: "center",
    fontStyle: "italic",
  },
  spinnerWrap: {
    display: "flex",
    flexDirection: "column",
    alignItems: "center",
    gap: 12,
    padding: "20px 0",
  },
  spinner: {
    width: 32,
    height: 32,
    border: "3px solid #e5e7eb",
    borderTopColor: "#2563eb",
    borderRadius: "50%",
    animation: "spin 0.8s linear infinite",
  },
  closeBtn: {
    background: "none",
    border: "none",
    fontSize: "1.1rem",
    cursor: "pointer",
    color: "#6b7280",
    lineHeight: 1,
    padding: 4,
  },
};
