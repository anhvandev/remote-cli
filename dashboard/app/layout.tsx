import { Chivo_Mono } from "next/font/google";
import "./globals.css";

const chivo = Chivo_Mono({ subsets: ["latin", "vietnamese"], weight: ["400", "500", "800"], variable: "--font-chivo" });

export const metadata = { title: "Remote CLI" };

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="vi" className={chivo.variable}>
      <body className="font-sans">
        <div className="flex min-h-screen flex-col md:flex-row">{children}</div>
      </body>
    </html>
  );
}
