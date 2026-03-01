import { readFileSync } from "node:fs";

import { ReadStdin } from "../stdio/index.ts";

export interface ReadOptions {
  Data?: string;
  File?: string;
  Label?: string;
  AllowEmpty?: boolean;
}

export function Read(opts: ReadOptions): string {
  const label = opts.Label || "JSON input";
  const data = opts.Data;
  const file = opts.File;
  const hasData = data !== undefined && data !== "";
  const hasFile = file !== undefined && file !== "";

  let raw = "";
  if (hasData && hasFile) {
    throw new Error(`${label}: use either --data or --file`);
  }

  if (hasData) {
    raw = data;
  } else if (hasFile) {
    try {
      raw = readFileSync(file, "utf8");
    } catch (error) {
      throw new Error(`${label}: read ${file}: ${errorMessage(error)}`);
    }
  } else {
    try {
      raw = ReadStdin();
    } catch (error) {
      throw new Error(`${label}: read stdin: ${errorMessage(error)}`);
    }
  }

  raw = raw.trim();
  if (raw === "" && opts.AllowEmpty) {
    return "null";
  }

  if (raw === "") {
    throw new Error(`${label}: empty payload`);
  }

  try {
    globalThis.JSON.parse(raw);
  } catch (error) {
    throw new Error(`${label}: invalid JSON: ${errorMessage(error)}`);
  }

  return raw;
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
