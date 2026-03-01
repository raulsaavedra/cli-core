const RFC3339_PATTERN =
  /^(\d{4}-\d{2}-\d{2})T(\d{2}:\d{2}:\d{2})(?:\.\d+)?(?:Z|[+-]\d{2}:\d{2})$/;

function formatDate(value: string): string {
  return formatWith(value, "date");
}

function formatDateTime(value: string): string {
  return formatWith(value, "datetime");
}

export function Truncate(text: string, max: number): string {
  if (max <= 0 || text.length <= max) {
    return text;
  }

  if (max <= 3) {
    return text.slice(0, max);
  }

  return `${text.slice(0, max - 3).trim()}...`;
}

function formatWith(value: string, layout: "date" | "datetime"): string {
  if (value === "") {
    return value;
  }

  const parsed = parseRFC3339(value);
  if (!parsed) {
    return value;
  }

  if (layout === "date") {
    return parsed.datePart;
  }

  return `${parsed.datePart} ${parsed.timePart}`;
}

function parseRFC3339(
  value: string,
): { datePart: string; timePart: string } | null {
  const match = RFC3339_PATTERN.exec(value);
  if (!match) {
    return null;
  }

  const timestamp = globalThis.Date.parse(value);
  if (Number.isNaN(timestamp)) {
    return null;
  }

  return {
    datePart: match[1],
    timePart: match[2],
  };
}

export { formatDate as Date, formatDateTime as DateTime };
