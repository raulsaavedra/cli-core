import { readFileSync } from "node:fs";

export function ReadStdin(): string {
  const data = readFileSync(0, "utf8");
  return data.trim();
}
