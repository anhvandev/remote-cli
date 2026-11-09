import { Nav } from "../nav";

export default function AppLayout({ children }: { children: React.ReactNode }) {
  return (
    <>
      <Nav />
      <main className="min-w-0 flex-1 px-5 py-6 md:px-8">
        <div className="mx-auto max-w-6xl">{children}</div>
      </main>
    </>
  );
}
