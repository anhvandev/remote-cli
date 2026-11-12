import { card, cardHead, title } from "../../styles";
import { UploadForm } from "./upload-form";

export default function UpdatesPage() {
  return (
    <section className={card} aria-label="Cập nhật Agent">
      <div className={cardHead}>
        <h3 className={title}>Cập nhật Agent</h3>
      </div>
      <div className="grid gap-5 p-5">
        <UploadForm />
      </div>
    </section>
  );
}
