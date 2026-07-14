import { describe, expect, it } from "bun:test";
import { chunkMarkdown } from "../utils/ai/chunk-markdown";

const MAX = 2000;

const COMBINING_ACUTE = "́";

function fenceCount(text: string): number {
  return (text.match(/^(?:>\s*)*`{3,}/gm) ?? []).length;
}

function stripWhitespace(text: string): string {
  return text.replace(/\s/g, "");
}

describe("chunkMarkdown", () => {
  it("returns nothing for empty or blank input", () => {
    expect(chunkMarkdown("")).toEqual([]);
    expect(chunkMarkdown("   \n  ")).toEqual([]);
  });

  it("returns short text unchanged", () => {
    expect(chunkMarkdown("hello world")).toEqual(["hello world"]);
  });

  it("never emits a chunk longer than the Discord limit", () => {
    const markdown = Array.from(
      { length: 200 },
      (_, i) => `## Heading ${i}\n\n${"word ".repeat(60)}\n`,
    ).join("\n");

    const chunks = chunkMarkdown(markdown);

    expect(chunks.length).toBeGreaterThan(1);
    for (const chunk of chunks) {
      expect(chunk.length).toBeLessThanOrEqual(MAX);
    }
  });

  it("clamps a caller-supplied maxLength above the Discord limit", () => {
    const chunks = chunkMarkdown("x ".repeat(4000), 5000);

    expect(chunks.length).toBeGreaterThan(1);
    for (const chunk of chunks) {
      expect(chunk.length).toBeLessThanOrEqual(MAX);
    }
  });

  it("re-fences every chunk of an oversized code block", () => {
    const code = Array.from(
      { length: 400 },
      (_, i) => `let value_${i} = compute(${i});`,
    ).join("\n");
    const markdown = "Here you go:\n\n```rust\n" + code + "\n```\n";

    const chunks = chunkMarkdown(markdown);
    const codeChunks = chunks.filter((chunk) => chunk.includes("```"));

    expect(codeChunks.length).toBeGreaterThan(1);
    for (const chunk of codeChunks) {
      expect(chunk.length).toBeLessThanOrEqual(MAX);
      // Balanced open + close fence, so no chunk leaks unterminated code formatting.
      expect(fenceCount(chunk) % 2).toBe(0);
      expect(chunk.trimEnd().endsWith("```")).toBe(true);
    }
    expect(codeChunks[0].startsWith("```rust")).toBe(true);
    expect(codeChunks[1].startsWith("```rust")).toBe(true);
  });

  it("preserves all code lines across the re-fenced chunks", () => {
    const lines = Array.from({ length: 300 }, (_, i) => `line_${i}();`);
    const markdown = "```ts\n" + lines.join("\n") + "\n```";

    const rejoined = chunkMarkdown(markdown)
      .join("\n")
      .replace(/^`{3,}\w*$/gm, "");

    for (const line of lines) {
      expect(rejoined).toContain(line);
    }
  });

  it("splits a single oversized code line without breaking the fence", () => {
    const markdown = "```\n" + "a".repeat(6000) + "\n```";

    const chunks = chunkMarkdown(markdown);

    expect(chunks.length).toBeGreaterThan(2);
    for (const chunk of chunks) {
      expect(chunk.length).toBeLessThanOrEqual(MAX);
      expect(fenceCount(chunk) % 2).toBe(0);
    }
  });

  it("escapes the fence when the code body contains a triple backtick", () => {
    const body = Array.from(
      { length: 300 },
      (_, i) => `let fence_${i} = "\`\`\`";`,
    ).join("\n");

    const chunks = chunkMarkdown("```rust\n" + body + "\n```");

    expect(chunks.length).toBeGreaterThan(1);
    for (const chunk of chunks) {
      expect(chunk.startsWith("````rust")).toBe(true);
      expect(chunk.trimEnd().endsWith("````")).toBe(true);
      expect(chunk.length).toBeLessThanOrEqual(MAX);
    }
  });

  it("balances fences nested inside an oversized blockquote", () => {
    const body = Array.from(
      { length: 350 },
      (_, i) => `> const quoted_${i} = ${i};`,
    ).join("\n");
    const markdown = `> Example:\n> \`\`\`ts\n${body}\n> \`\`\``;

    const chunks = chunkMarkdown(markdown);

    expect(chunks.length).toBeGreaterThan(1);
    for (const chunk of chunks) {
      expect(chunk.length).toBeLessThanOrEqual(MAX);
      expect(fenceCount(chunk) % 2).toBe(0);
    }
  });

  it("does not split surrogate pairs", () => {
    const markdown = "\u{1F680}\u{1F389}".repeat(3000);

    const chunks = chunkMarkdown(markdown);

    expect(chunks.length).toBeGreaterThan(1);
    for (const chunk of chunks) {
      expect(chunk.length).toBeLessThanOrEqual(MAX);
      expect(/[\uD800-\uDBFF](?![\uDC00-\uDFFF])/.test(chunk)).toBe(false);
      expect(/(?<![\uD800-\uDBFF])[\uDC00-\uDFFF]/.test(chunk)).toBe(false);
    }
    // Whitespace at split boundaries is trimmed; no emoji may be lost.
    expect(stripWhitespace(chunks.join(""))).toBe(stripWhitespace(markdown));
  });

  it("keeps combining marks attached to their base character", () => {
    // The leading "x" offsets the base+mark pairs so a naive split lands on a mark.
    const markdown = "x" + ("e" + COMBINING_ACUTE).repeat(3000);

    const chunks = chunkMarkdown(markdown);

    expect(chunks.length).toBeGreaterThan(1);
    for (const chunk of chunks) {
      expect(chunk.length).toBeLessThanOrEqual(MAX);
      expect(/^\p{M}/u.test(chunk)).toBe(false);
    }
    expect(stripWhitespace(chunks.join(""))).toBe(stripWhitespace(markdown));
  });

  it("never emits a blank chunk", () => {
    const markdown = "\n\n\n" + "para\n\n".repeat(800);

    for (const chunk of chunkMarkdown(markdown)) {
      expect(chunk.trim().length).toBeGreaterThan(0);
    }
  });
});
