import "server-only";
import { revalidatePath } from "next/cache";
import { ApiError } from "@/lib/api";
import type { Result } from "./actions";

/** Runs an admin server action's API call and refreshes `path`; only a string result is shown (API response
 * bodies are not), and an API error becomes the Result's `error`. */
export async function run(path: string, fn: () => Promise<unknown>): Promise<Result> {
  try {
    const r = await fn();
    revalidatePath(path);
    return { message: typeof r === "string" ? r : undefined };
  } catch (e) {
    if (!(e instanceof ApiError)) throw e; // includes Next's redirect to /signin
    return { error: `${e.code}: ${e.message}` };
  }
}
