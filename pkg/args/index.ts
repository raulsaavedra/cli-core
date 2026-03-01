const INTEGER_PATTERN = /^[+-]?\d+$/;

export function ParseIntOption(value: string, label: string): number {
  if (!INTEGER_PATTERN.test(value)) {
    throw new Error(`${label} must be an integer`);
  }

  const parsed = Number(value);
  if (!Number.isSafeInteger(parsed)) {
    throw new Error(`${label} must be an integer`);
  }

  return parsed;
}

export function RequireExactlyOne(
  values: Record<string, boolean>,
  message: string,
): void {
  let count = 0;
  for (const present of Object.values(values)) {
    if (present) {
      count += 1;
    }
  }

  if (count !== 1) {
    throw new Error(message);
  }
}
