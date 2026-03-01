import { describe, expect, test } from "bun:test";

import {
  Date as FormatDate,
  DateTime as FormatDateTime,
  Truncate,
} from "./index.ts";

describe("Date", () => {
  test("formats RFC3339 strings to YYYY-MM-DD", () => {
    expect(FormatDate("2025-02-11T10:31:45Z")).toBe("2025-02-11");
    expect(FormatDate("2025-02-11T10:31:45-04:00")).toBe("2025-02-11");
  });

  test("returns original value when parsing fails", () => {
    expect(FormatDate("not-a-date")).toBe("not-a-date");
    expect(FormatDate("")).toBe("");
  });
});

describe("DateTime", () => {
  test("formats RFC3339 strings to YYYY-MM-DD HH:MM:SS", () => {
    expect(FormatDateTime("2025-02-11T10:31:45Z")).toBe("2025-02-11 10:31:45");
  });

  test("returns original value when parsing fails", () => {
    expect(FormatDateTime("not-a-date")).toBe("not-a-date");
  });
});

describe("Truncate", () => {
  test("truncates and appends ellipsis", () => {
    expect(Truncate("This is a very long sentence", 10)).toBe("This is...");
  });

  test("keeps source when max is zero or string already fits", () => {
    expect(Truncate("abc", 0)).toBe("abc");
    expect(Truncate("abc", 3)).toBe("abc");
  });
});
