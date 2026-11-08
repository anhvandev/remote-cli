import { Button } from "@/components/ui/button";
import * as act from "../actions";
import { Form } from "../form";
import { card, cardHead, title } from "../shared";

export default function EnrollmentPage() {
  return (
    <main className="min-w-0 flex-1 px-5 py-6 md:px-8">
      <div className="mx-auto max-w-6xl">
        <h2 className="mb-5 text-xl font-semibold tracking-tight">Quản trị</h2>
        <section className={card} aria-label="Enrollment Code">
          <div className={cardHead}>
            <h3 className={title}>Enrollment Code</h3>
            <Form action={act.createEnrollmentCode}>
              <Button size="sm">+ Tạo mã</Button>
            </Form>
          </div>
          <div className="px-5 py-4">
            <p className="text-sm text-muted-foreground">Mã chỉ hiện một lần; Server chỉ lưu bản băm.</p>
          </div>
        </section>
      </div>
    </main>
  );
}
