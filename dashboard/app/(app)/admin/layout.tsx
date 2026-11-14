/** Every page under Quản lý; every User may open them (ADR 0009). */
export default function AdminLayout({ children }: { children: React.ReactNode }) {
  return (
    <>
      <h2 className="mb-5 max-w-none border-b-2 border-ink pb-4 text-3xl font-extrabold tracking-tight">Quản lý</h2>
      {children}
    </>
  );
}
