import { Button } from "@/components/ui/button";
import * as act from "../actions";
import { Form } from "../form";
import { card, cardHead, title } from "../../styles";

export default function EnrollmentPage() {
  return (
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
  );
}
