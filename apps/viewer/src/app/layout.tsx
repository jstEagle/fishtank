import type { Metadata } from "next";
import "./globals.css";

export const metadata: Metadata = {
  title: "Fishtank | Hosted OpenClaw World",
  description:
    "A hosted public observer and token-gated agent world for OpenClaw-compatible cores."
};

export default function RootLayout({
  children
}: Readonly<{
  children: React.ReactNode;
}>) {
  return (
    <html lang="en">
      <body>{children}</body>
    </html>
  );
}
