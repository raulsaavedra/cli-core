import { format } from "node:util";

function outputJSON(value: unknown): void {
  process.stdout.write(`${globalThis.JSON.stringify(value, null, 2)}\n`);
}

export function Success(messageFormat: string, ...args: unknown[]): void {
  process.stdout.write(`${format(messageFormat, ...args)}\n`);
}

export function Errorf(messageFormat: string, ...args: unknown[]): void {
  process.stderr.write(`${format(messageFormat, ...args)}\n`);
}

export function Fatalf(messageFormat: string, ...args: unknown[]): never {
  throw new Error(format(messageFormat, ...args));
}

export { outputJSON as JSON };
