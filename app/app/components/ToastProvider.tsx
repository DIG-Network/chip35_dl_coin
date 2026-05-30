"use client";

import { Toaster } from "react-hot-toast";

export default function ToastProvider() {
  return (
    <Toaster
      position="bottom-right"
      toastOptions={{
        duration: 4000,
        style: {
          fontSize: "0.9rem",
          maxWidth: 420,
        },
      }}
    />
  );
}
