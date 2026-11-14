import { api, type AgentBuild } from "@/lib/api";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Table, TableBody, TableCell, TableRow } from "@/components/ui/table";
import * as act from "../actions";
import { Form } from "../form";
import { card, cardHead, title } from "../../styles";
import { UploadForm } from "./upload-form";

const size = (n: number) => (n < 1 << 20 ? `${Math.ceil(n / 1024)} KB` : `${(n / (1 << 20)).toFixed(1)} MB`);
const seg = encodeURIComponent;

export default async function UpdatesPage() {
  const builds = await api<AgentBuild[]>("/api/v1/agent-builds");
  // The API lists newest version first; the groups keep that order.
  const versions = Map.groupBy(builds, (b) => b.version);
  return (
    <section className={card} aria-label="Cập nhật Agent">
      <div className={cardHead}>
        <h3 className={title}>Cập nhật Agent</h3>
      </div>
      <div className="grid gap-5 p-5">
        <UploadForm />
        {[...versions].map(([version, targets]) => (
          <section key={version} aria-label={`Phiên bản ${version}`} className="grid gap-1">
            <div className="flex items-center justify-between">
              <h4 className="text-lg font-extrabold">{version}</h4>
              <Form action={act.deleteBuild} confirm={`Xoá mọi build của phiên bản ${version}?`}>
                <input type="hidden" name="version" value={version} />
                <Button size="xs" variant="ghost" className="text-destructive">
                  Xoá phiên bản
                </Button>
              </Form>
            </div>
            <Table>
              <TableBody>
                {targets.map((b) => (
                  <TableRow key={b.target}>
                    <TableCell>
                      {b.target}{" "}
                      {b.latest && (
                        <Badge>
                          Latest
                        </Badge>
                      )}
                    </TableCell>
                    <TableCell className="text-muted-foreground">{size(b.size)}</TableCell>
                    <TableCell className="text-xs text-muted-foreground" title={b.sha256}>
                      {b.sha256.slice(0, 12)}
                    </TableCell>
                    <TableCell className="text-muted-foreground">
                      {b.uploaded_by}
                      <span className="ml-3">{b.uploaded_at.slice(0, 16).replace("T", " ")}</span>
                    </TableCell>
                    <TableCell>
                      <div className="flex items-center justify-end gap-1.5">
                        <Button size="xs" variant="outline" asChild>
                          <a href={`/api/agent-builds/${seg(version)}/${seg(b.target)}`} download={`rcli-${version}-${b.target}`}>
                            Tải về
                          </a>
                        </Button>
                        <Form action={act.deleteBuild} confirm={`Xoá build ${version} cho ${b.target}?`}>
                          <input type="hidden" name="version" value={version} />
                          <input type="hidden" name="target" value={b.target} />
                          <Button size="xs" variant="ghost" className="text-destructive">
                            Xoá
                          </Button>
                        </Form>
                      </div>
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </section>
        ))}
      </div>
    </section>
  );
}
