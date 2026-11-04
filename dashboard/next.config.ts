import type { NextConfig } from "next";

// standalone: a self-contained server for the Docker image.
const config: NextConfig = { output: "standalone" };
export default config;
