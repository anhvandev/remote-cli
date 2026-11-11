import { forward } from "@/lib/forward";

// The whole Controller API through the Dashboard URL (ADR 0006).
const all = (req: Request) => forward(req);

export { all as GET, all as POST, all as PUT, all as PATCH, all as DELETE, all as HEAD };
