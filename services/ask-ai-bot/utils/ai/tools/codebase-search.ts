import fs from "fs";
import path from "path";
import { logger } from "../../logger";

export interface CodeSearchResult {
  filePath: string;
  line: number;
  content: string;
  context: string;
}

const SOURCE_EXTENSIONS = new Set([
  ".rs",
  ".ts",
  ".tsx",
  ".js",
  ".jsx",
  ".json",
  ".toml",
  ".yaml",
  ".yml",
  ".css",
  ".scss",
  ".html",
  ".md",
  ".sql",
  ".sh",
  ".mts",
]);

const IGNORED_DIRS = new Set([
  "node_modules",
  "target",
  "dist",
  "out",
  ".vite",
  ".git",
  "build",
  "coverage",
]);

export const MAX_SEARCH_RESULTS = 50;
export const MAX_LIST_ENTRIES = 200;
const MAX_QUERY_LENGTH = 200;
const MAX_FILES_SCANNED = 5000;
const MAX_FILE_BYTES = 512 * 1024;
const MAX_LINE_LENGTH = 500;
const MAX_DIR_DEPTH = 20;
const SEARCH_BUDGET_MS = 10_000;

interface SearchBudget {
  deadline: number;
  filesScanned: number;
}

function budgetExhausted(budget: SearchBudget): boolean {
  return (
    budget.filesScanned >= MAX_FILES_SCANNED || Date.now() > budget.deadline
  );
}

function getCodebaseDir(): string {
  return process.env.CODEBASE_PATH || path.join(process.cwd(), "../..");
}

function getSearchableDirs(): { name: string; path: string }[] {
  const base = path.resolve(getCodebaseDir());
  return [
    { name: "ui", path: path.join(base, "ui") },
    { name: "crates", path: path.join(base, "crates") },
  ];
}

function getSafeSearchableDirs(): { name: string; path: string }[] {
  return getSearchableDirs().flatMap((dir) => {
    if (!fs.existsSync(dir.path) || fs.lstatSync(dir.path).isSymbolicLink()) {
      return [];
    }

    return [{ name: dir.name, path: fs.realpathSync(dir.path) }];
  });
}

function shouldSkipDir(dirName: string): boolean {
  return IGNORED_DIRS.has(dirName);
}

function isSourceFile(fileName: string): boolean {
  const ext = path.extname(fileName).toLowerCase();
  return SOURCE_EXTENSIONS.has(ext);
}

function getContextLines(
  lines: string[],
  matchLine: number,
  contextSize: number = 2,
): string {
  const start = Math.max(0, matchLine - contextSize);
  const end = Math.min(lines.length - 1, matchLine + contextSize);
  const contextLines: string[] = [];

  for (let i = start; i <= end; i++) {
    const prefix = i === matchLine ? ">" : " ";
    contextLines.push(`${prefix} ${i + 1}: ${truncateLine(lines[i])}`);
  }

  return contextLines.join("\n");
}

function truncateLine(line: string): string {
  if (line.length <= MAX_LINE_LENGTH) return line;
  return line.slice(0, MAX_LINE_LENGTH) + "…";
}

function searchInFile(
  filePath: string,
  needle: string,
  baseDir: string,
  budget: SearchBudget,
  remaining: number,
): CodeSearchResult[] {
  const results: CodeSearchResult[] = [];

  try {
    if (fs.statSync(filePath).size > MAX_FILE_BYTES) return results;

    const content = fs.readFileSync(filePath, "utf-8");
    const lines = content.split("\n");

    for (let i = 0; i < lines.length; i++) {
      if (results.length >= remaining || budgetExhausted(budget)) break;

      if (lines[i].toLowerCase().includes(needle)) {
        results.push({
          filePath: path.relative(baseDir, filePath),
          line: i + 1,
          content: truncateLine(lines[i].trim()),
          context: getContextLines(lines, i),
        });
      }
    }
  } catch {
    // Skip files that can't be read (binary, permissions, etc.)
  }

  return results;
}

function walkAndSearch(
  dir: string,
  needle: string,
  baseDir: string,
  results: CodeSearchResult[],
  maxResults: number,
  budget: SearchBudget,
  depth: number = 0,
): void {
  if (results.length >= maxResults || depth > MAX_DIR_DEPTH) return;
  if (budgetExhausted(budget)) return;

  try {
    const entries = fs.readdirSync(dir, { withFileTypes: true });

    for (const entry of entries) {
      if (results.length >= maxResults || budgetExhausted(budget)) return;

      if (entry.isSymbolicLink()) {
        continue;
      }
      if (entry.isDirectory()) {
        if (shouldSkipDir(entry.name)) continue;
        walkAndSearch(
          path.join(dir, entry.name),
          needle,
          baseDir,
          results,
          maxResults,
          budget,
          depth + 1,
        );
      } else if (entry.isFile() && isSourceFile(entry.name)) {
        budget.filesScanned++;
        const fileResults = searchInFile(
          path.join(dir, entry.name),
          needle,
          baseDir,
          budget,
          maxResults - results.length,
        );
        results.push(...fileResults);
      }
    }
  } catch (error) {
    logger.error(`Error walking directory ${dir}:`, error);
  }
}

/**
 * Literal, case-insensitive substring search. The query is model-supplied, so it
 * is never compiled as a regex — a pattern like `(a+)+$` would backtrack the
 * event loop into the ground with no way to interrupt it.
 */
export function searchCodebase(
  query: string,
  limit: number = 20,
  scope?: string,
): CodeSearchResult[] {
  const needle = query.trim().slice(0, MAX_QUERY_LENGTH).toLowerCase();
  if (!needle) return [];

  const maxResults = Math.max(
    1,
    Math.min(Math.floor(limit), MAX_SEARCH_RESULTS),
  );
  const budget: SearchBudget = {
    deadline: Date.now() + SEARCH_BUDGET_MS,
    filesScanned: 0,
  };

  const allResults: CodeSearchResult[] = [];
  const baseDir = path.resolve(getCodebaseDir());

  for (const dir of getSafeSearchableDirs()) {
    if (scope && dir.name !== scope) continue;

    if (!fs.existsSync(dir.path)) {
      logger.warn(`Codebase directory not found: ${dir.path}`);
      continue;
    }

    walkAndSearch(dir.path, needle, baseDir, allResults, maxResults, budget);
  }

  logger.verbose(
    `Code search for "${needle}" returned ${allResults.length} results`,
  );
  return allResults.slice(0, maxResults);
}

/** Resolves a repo-relative path, rejecting anything outside ui/ and crates/. */
function resolveSearchablePath(directory: string): string {
  const baseDir = fs.realpathSync(path.resolve(getCodebaseDir()));
  const targetDir = path.resolve(baseDir, directory);

  if (!fs.existsSync(targetDir) || fs.lstatSync(targetDir).isSymbolicLink()) {
    throw new Error(`Directory not found or unsafe: ${directory}`);
  }
  const realTarget = fs.realpathSync(targetDir);

  const allowed = getSafeSearchableDirs().some(
    (dir) =>
      realTarget === dir.path || realTarget.startsWith(dir.path + path.sep),
  );

  if (!allowed) {
    throw new Error("Invalid path - only ui/ and crates/ can be listed");
  }

  return realTarget;
}

export function listCodebaseFiles(
  directory: string,
): { filePath: string; isDirectory: boolean }[] {
  const targetDir = resolveSearchablePath(directory);

  if (!fs.existsSync(targetDir)) {
    throw new Error(`Directory not found: ${directory}`);
  }

  if (!fs.statSync(targetDir).isDirectory()) {
    throw new Error(`Not a directory: ${directory}`);
  }

  try {
    const entries = fs.readdirSync(targetDir, { withFileTypes: true });
    return entries
      .filter((entry) => !entry.isSymbolicLink() && !shouldSkipDir(entry.name))
      .map((entry) => ({
        filePath: path.join(directory, entry.name),
        isDirectory: entry.isDirectory(),
      }))
      .sort((a, b) => {
        if (a.isDirectory !== b.isDirectory) return a.isDirectory ? -1 : 1;
        return a.filePath.localeCompare(b.filePath);
      })
      .slice(0, MAX_LIST_ENTRIES);
  } catch {
    throw new Error(`Failed to list directory: ${directory}`);
  }
}
