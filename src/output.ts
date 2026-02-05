export interface OutputOptions<T = unknown> {
  json?: boolean;
  emptyMessage?: string;
  formatItem?: (item: T) => string;
}

export function output<T>(data: T | T[], options: OutputOptions<T> = {}): void {
  if (options.json) {
    console.log(JSON.stringify(data, null, 2));
    return;
  }

  const formatItem =
    options.formatItem ?? ((item: T) => defaultFormatItem(item));

  if (Array.isArray(data)) {
    if (data.length === 0) {
      if (options.emptyMessage) {
        console.log(options.emptyMessage);
      }
      return;
    }
    for (const item of data) {
      console.log(formatItem(item));
    }
    return;
  }

  console.log(formatItem(data));
}

export function error(message: string): never {
  console.error(`Error: ${message}`);
  process.exit(1);
}

export function success(message: string): void {
  console.log(message);
}

function defaultFormatItem(item: unknown): string {
  if (typeof item === "object" && item !== null) {
    return formatRecord(item as Record<string, unknown>);
  }
  return String(item);
}

function formatRecord(record: Record<string, unknown>): string {
  return Object.entries(record)
    .map(([key, value]) => `${key}: ${String(value)}`)
    .join(", ");
}
