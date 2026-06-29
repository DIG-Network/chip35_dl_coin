import type { Metadata } from "next";
import "./globals.css";
import WalletProvider from "./components/WalletProvider";
import ToastProvider from "./components/ToastProvider";

export const metadata: Metadata = {
  title: "DIG Network — CHIP-0035 Store Demo",
  description:
    "A DIG Network demo: mint a store, advance it to a new capsule, and melt it on Chia via the DIG Browser or Sage Wallet (WalletConnect).",
};

export default function RootLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  return (
    <html lang="en">
      <body>
        <WalletProvider>
          {children}
          <ToastProvider />
        </WalletProvider>
      </body>
    </html>
  );
}
