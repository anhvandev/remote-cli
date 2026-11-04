import { proxy } from "@/lib/api";

export const GET = () => proxy("/api/v1/ice-servers");
