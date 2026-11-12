/** Every page under Quản lý; every User may open them (ADR 0009). */
export default function AdminLayout({ children }: { children: React.ReactNode }) {
  return (
    <>
      <h2 className="mb-5 text-xl font-semibold tracking-tight">Quản lý</h2>
      {children}
    </>
  );
}
