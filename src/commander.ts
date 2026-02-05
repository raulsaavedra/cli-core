type Optionable<T> = {
  option: (...args: unknown[]) => T;
};

export function collect(value: string, previous: string[]): string[] {
  return previous.concat([value]);
}

export function withJsonOption<T extends Optionable<T>>(
  command: T,
  description = "Output as JSON"
): T {
  return command.option("--json", description);
}
