import { readFile } from "node:fs/promises";

export type JsonSchema<T> = {
  parse: (input: unknown) => T;
};

export interface JsonInputOptions<T> {
  data?: string;
  file?: string;
  label?: string;
  schema?: JsonSchema<T>;
  allowEmpty?: boolean;
}

export async function readJsonInput<T = unknown>(
  options: JsonInputOptions<T>
): Promise<T> {
  const label = options.label ?? "JSON";
  const sources = [options.data, options.file].filter(
    (value) => value !== undefined
  );
  if (sources.length === 0) {
    throw new Error(`Provide ${label} via --data or --file.`);
  }
  if (sources.length > 1) {
    throw new Error(`Specify only one of --data or --file.`);
  }

  let content = "";
  if (options.data !== undefined) {
    content = options.data;
  } else if (options.file !== undefined) {
    try {
      content = await readFile(options.file, "utf-8");
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      throw new Error(`Unable to read file '${options.file}': ${message}`);
    }
  }

  if (!options.allowEmpty && !content.trim()) {
    throw new Error(`${label} JSON cannot be empty.`);
  }

  let parsed: unknown;
  try {
    parsed = JSON.parse(content);
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    if (options.data !== undefined) {
      throw new Error(
        `Invalid ${label} JSON: ${message}. If using --data, make sure quotes inside strings are escaped (\\\") or removed.`
      );
    }
    throw new Error(`Invalid ${label} JSON: ${message}`);
  }

  if (options.schema) {
    return options.schema.parse(parsed);
  }

  return parsed as T;
}
