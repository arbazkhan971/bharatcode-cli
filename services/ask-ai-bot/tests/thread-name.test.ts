import { describe, expect, it } from "bun:test";
import { buildThreadName } from "../events/messageCreate";

const MAX_THREAD_NAME_LENGTH = 100;

describe("buildThreadName", () => {
  it("uses the message content when it fits", () => {
    expect(buildThreadName("How do I configure BharatCode?", "ada")).toBe(
      "How do I configure BharatCode?",
    );
  });

  it("falls back to the author when the message has no text", () => {
    expect(buildThreadName("", "ada")).toBe("Question from ada");
    expect(buildThreadName("   \n ", "ada")).toBe("Question from ada");
  });

  it("truncates long content to Discord's limit", () => {
    const name = buildThreadName("q".repeat(500), "ada");

    expect(name.length).toBe(MAX_THREAD_NAME_LENGTH);
    expect(name.endsWith("...")).toBe(true);
  });

  it("counts astral characters as one and never splits a surrogate pair", () => {
    const name = buildThreadName("\u{1F680}".repeat(200), "ada");

    expect(Array.from(name).length).toBe(MAX_THREAD_NAME_LENGTH);
    expect(/[\uD800-\uDBFF](?![\uDC00-\uDFFF])/.test(name)).toBe(false);
    expect(/(?<![\uD800-\uDBFF])[\uDC00-\uDFFF]/.test(name)).toBe(false);
  });

  it("collapses newlines that Discord would reject in a thread name", () => {
    expect(buildThreadName("line one\nline two", "ada")).toBe(
      "line one line two",
    );
  });

  it("caps the fallback name for a very long username", () => {
    const name = buildThreadName("", "u".repeat(200));

    expect(Array.from(name).length).toBe(MAX_THREAD_NAME_LENGTH);
  });
});
