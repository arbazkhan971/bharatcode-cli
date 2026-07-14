import { afterAll, beforeAll, describe, expect, it } from "bun:test";
import fs from "fs";
import os from "os";
import path from "path";
import {
  MAX_SEARCH_RESULTS,
  listCodebaseFiles,
  searchCodebase,
} from "../utils/ai/tools/codebase-search";

let repoDir: string;
let previousCodebasePath: string | undefined;

function write(relativePath: string, content: string): void {
  const target = path.join(repoDir, relativePath);
  fs.mkdirSync(path.dirname(target), { recursive: true });
  fs.writeFileSync(target, content);
}

beforeAll(() => {
  previousCodebasePath = process.env.CODEBASE_PATH;
  repoDir = fs.mkdtempSync(path.join(os.tmpdir(), "ask-ai-bot-"));
  process.env.CODEBASE_PATH = repoDir;

  write(
    "crates/bharatcode-core/src/agent.rs",
    [
      "impl Agent {",
      "    pub fn create_session(&self) {}",
      "}",
      "let total = compute(1);",
    ].join("\n"),
  );

  write(
    "ui/text/src/App.tsx",
    ["export function App() {", "  return null;", "}"].join("\n"),
  );

  // A line shaped to detonate a backtracking regex like (a+)+$ if one were compiled.
  write("crates/bharatcode-core/src/evil.rs", "// " + "a".repeat(4000) + "!");

  write(
    "crates/bharatcode-core/src/many.rs",
    Array.from({ length: 200 }, (_, i) => `let needle_${i} = 1;`).join("\n"),
  );

  write("secrets.md", "SUPER_SECRET_TOKEN=abc123");
  write("crates/bharatcode-core/node_modules/pkg/index.ts", "create_session ignored");
});

afterAll(() => {
  if (previousCodebasePath === undefined) {
    delete process.env.CODEBASE_PATH;
  } else {
    process.env.CODEBASE_PATH = previousCodebasePath;
  }
  fs.rmSync(repoDir, { recursive: true, force: true });
});

describe("searchCodebase", () => {
  it("finds a literal substring, case-insensitively", () => {
    const results = searchCodebase("CREATE_SESSION");

    expect(results.length).toBe(1);
    expect(results[0].filePath).toBe(
      path.join("crates", "bharatcode-core", "src", "agent.rs"),
    );
    expect(results[0].line).toBe(2);
    expect(results[0].context).toContain("> 2:");
  });

  it("treats regex metacharacters as literal text", () => {
    // Would match as a regex, must not match as a literal.
    expect(searchCodebase("impl.*Agent")).toEqual([]);
    // Would be a regex group, must match the literal parentheses.
    expect(searchCodebase("compute(1)").length).toBe(1);
  });

  it("does not backtrack on a catastrophic pattern", () => {
    const start = Date.now();

    const results = searchCodebase("(a+)+$");

    expect(results).toEqual([]);
    expect(Date.now() - start).toBeLessThan(2000);
  });

  it("ignores an empty query instead of matching every line", () => {
    expect(searchCodebase("   ")).toEqual([]);
  });

  it("clamps the result limit", () => {
    const results = searchCodebase("needle_", 10_000);

    expect(results.length).toBe(MAX_SEARCH_RESULTS);
  });

  it("honours a lower caller limit", () => {
    expect(searchCodebase("needle_", 3).length).toBe(3);
  });

  it("respects scope", () => {
    expect(searchCodebase("export function App", 20, "crates")).toEqual([]);
    expect(searchCodebase("export function App", 20, "ui").length).toBe(1);
  });

  it("skips ignored directories", () => {
    const results = searchCodebase("create_session");

    expect(results.some((r) => r.filePath.includes("node_modules"))).toBe(
      false,
    );
  });
});

describe("listCodebaseFiles", () => {
  it("lists entries inside a searchable root", () => {
    const entries = listCodebaseFiles("crates/bharatcode-core/src");

    expect(entries.map((e) => path.basename(e.filePath))).toContain("agent.rs");
  });

  it("rejects directory traversal", () => {
    expect(() => listCodebaseFiles("../../etc")).toThrow(/Invalid path/);
    expect(() => listCodebaseFiles("crates/../../..")).toThrow(/Invalid path/);
  });

  it("rejects paths outside ui/ and crates/", () => {
    expect(() => listCodebaseFiles(".")).toThrow(/Invalid path/);
    expect(() => listCodebaseFiles("")).toThrow(/Invalid path/);
  });

  it("rejects an absolute path escaping the repo", () => {
    expect(() => listCodebaseFiles("/etc")).toThrow(/Invalid path/);
  });

  it("throws for a missing directory inside a searchable root", () => {
    expect(() => listCodebaseFiles("crates/nope")).toThrow(/not found/i);
  });
});
