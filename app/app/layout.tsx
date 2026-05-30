import type { Metadata } from "next";
import "./globals.css";
import WalletProvider from "./components/WalletProvider";
import ToastProvider from "./components/ToastProvider";

export const metadata: Metadata = {
  title: "CHIP-0035 DataLayer Store Demo",
  description:
    "List, mint, update, and delete CHIP-0035 DataLayer stores on Chia via Sage Wallet and WalletConnect.",
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
