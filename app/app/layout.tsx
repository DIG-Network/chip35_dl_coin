import type { Metadata } from "next";

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
      <body style={{ margin: 0, fontFamily: "sans-serif", minHeight: "100vh" }}>
        {children}
      </body>
    </html>
  );
}
