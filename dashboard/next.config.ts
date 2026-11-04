import type { NextConfig } from "next";

// standalone: a self-contained server for the Docker image.
// bodySizeLimit: Agent builds are uploaded through a server action (the Server accepts up to 200 MiB).
const config: NextConfig = { output: "standalone", experimental: { serverActions: { bodySizeLimit: "200mb" } } };
export default config;
