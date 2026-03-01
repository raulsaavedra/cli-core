import { describe, expect, test } from "bun:test";

import { ParseIntOption, RequireExactlyOne } from "./index.ts";

describe("ParseIntOption", () => {
  test("parses valid integer strings", () => {
    expect(ParseIntOption("42", "count")).toBe(42);
    expect(ParseIntOption("-7", "offset")).toBe(-7);
  });

  test("throws on invalid integers", () => {
    expect(() => ParseIntOption("12.4", "count")).toThrow(
      "count must be an integer",
    );
    expect(() => ParseIntOption("12abc", "count")).toThrow(
      "count must be an integer",
    );
  });
});

describe("RequireExactlyOne", () => {
  test("allows exactly one truthy value", () => {
    expect(() =>
      RequireExactlyOne(
        {
          foo: false,
          bar: true,
          baz: false,
        },
        "pick exactly one",
      ),
    ).not.toThrow();
  });

  test("throws when zero or multiple values are true", () => {
    expect(() =>
      RequireExactlyOne({ foo: false, bar: false }, "pick exactly one"),
    ).toThrow("pick exactly one");
    expect(() =>
      RequireExactlyOne({ foo: true, bar: true }, "pick exactly one"),
    ).toThrow("pick exactly one");
  });
});
