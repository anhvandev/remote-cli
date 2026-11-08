import { Inter } from "next/font/google";
import "./globals.css";

const inter = Inter({ subsets: ["latin", "vietnamese"], variable: "--font-inter" });

export const metadata = { title: "Remote CLI" };

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="vi" className={`dark ${inter.variable}`}>
      <body className="font-sans">
        <div className="flex min-h-screen flex-col md:flex-row">{children}</div>
      </body>
    </html>
  );
}
