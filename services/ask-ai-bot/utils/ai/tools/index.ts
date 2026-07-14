import { tool } from "ai";
import { z } from "zod";
import { logger } from "../../logger";
import {
  MAX_SEARCH_RESULTS,
  listCodebaseFiles,
  searchCodebase,
} from "./codebase-search";
import { viewCodebaseFiles } from "./codebase-viewer";
import { searchDocs } from "./docs-search";
import { viewDocs } from "./docs-viewer";

const MAX_FILE_PATHS = 10;
const MAX_PATH_LENGTH = 300;
const MAX_DOC_RESULTS = 30;
const MAX_DOC_LINES = 1500;
const MAX_CODE_LINES = 500;
const MAX_START_LINE = 100_000;

const querySchema = z.string().min(1).max(200);

const filePathsSchema = z.union([
  z.string().min(1).max(MAX_PATH_LENGTH),
  z.array(z.string().min(1).max(MAX_PATH_LENGTH)).min(1).max(MAX_FILE_PATHS),
]);

const startLineSchema = z.number().int().min(0).max(MAX_START_LINE);

export const aiTools = {
  search_docs: tool({
    description: "Search the BharatCode documentation for relevant information",
    inputSchema: z.object({
      query: querySchema.describe(
        "Search query for the documentation (example: 'sessions', 'tool management')",
      ),
      limit: z
        .number()
        .int()
        .min(1)
        .max(MAX_DOC_RESULTS)
        .optional()
        .describe(
          `Maximum number of results to return (default 15, max ${MAX_DOC_RESULTS})`,
        ),
    }),
    execute: async ({ query, limit = 15 }) => {
      const results = searchDocs(query, limit);
      logger.verbose(
        `Searched docs for "${query}", found ${results.length} results`,
      );

      if (results.length === 0) {
        return "No relevant documentation found for your query. Try different keywords.";
      }

      return results
        .map(
          (r) =>
            `**${r.fileName}** (${r.filePath})\nPreview: ${r.preview}\nWeb URL: <${r.webUrl}>`,
        )
        .join("\n\n");
    },
  }),
  view_docs: tool({
    description: "View documentation file(s)",
    inputSchema: z.object({
      filePaths: filePathsSchema.describe(
        `Path or array of up to ${MAX_FILE_PATHS} paths to documentation files (example: 'quickstart.md' or ['guides/managing-projects.md', 'mcp/asana-mcp.md'])`,
      ),
      startLine: startLineSchema
        .optional()
        .describe("Starting line number (0-indexed, default 0)"),
      lineCount: z
        .number()
        .int()
        .min(1)
        .max(MAX_DOC_LINES)
        .optional()
        .describe(
          `Number of lines to show (default ${MAX_DOC_LINES}, max ${MAX_DOC_LINES})`,
        ),
    }),
    execute: async ({ filePaths, startLine = 0, lineCount = MAX_DOC_LINES }) => {
      try {
        const result = viewDocs(filePaths, startLine, lineCount);
        const count = Array.isArray(filePaths) ? filePaths.length : 1;
        logger.verbose(`Viewed ${count} documentation file(s)`);
        return result;
      } catch (error) {
        const errorMsg =
          error instanceof Error ? error.message : "Unknown error";
        logger.error(`Error viewing docs: ${errorMsg}`);
        return `Error viewing documentation: ${errorMsg}`;
      }
    },
  }),
  search_codebase: tool({
    description:
      "Search the BharatCode source code for a literal, case-insensitive substring. Searches across ui/ and crates/. Use this to find function definitions, struct/type definitions, imports, or error messages. This is a plain text search, not a regex search - pass the exact text you expect to appear in the source (example: 'fn create_session', not 'fn.*session').",
    inputSchema: z.object({
      query: querySchema.describe(
        "Literal text to search for in the codebase (example: 'fn create_session', 'struct Provider', 'impl Agent')",
      ),
      limit: z
        .number()
        .int()
        .min(1)
        .max(MAX_SEARCH_RESULTS)
        .optional()
        .describe(
          `Maximum number of results to return (default 20, max ${MAX_SEARCH_RESULTS})`,
        ),
      scope: z
        .enum(["ui", "crates"])
        .optional()
        .describe(
          "Limit search to a specific area: 'ui' for the terminal UI, 'crates' for Rust code. Omit to search everything.",
        ),
    }),
    execute: async ({ query, limit = 20, scope }) => {
      try {
        const results = searchCodebase(query, limit, scope);

        if (results.length === 0) {
          return "No matches found in the codebase. Try a different pattern or broader search.";
        }

        return results
          .map(
            (r) => `**${r.filePath}:${r.line}**\n\`\`\`\n${r.context}\n\`\`\``,
          )
          .join("\n\n");
      } catch (error) {
        const errorMsg =
          error instanceof Error ? error.message : "Unknown error";
        logger.error(`Error searching codebase: ${errorMsg}`);
        return `Error searching codebase: ${errorMsg}`;
      }
    },
  }),
  view_codebase: tool({
    description:
      "View source code file(s) from the BharatCode codebase. Paths are relative to the repository root (e.g., 'crates/bharatcode-core/src/agents/agent.rs' or 'ui/text/src/tui.js').",
    inputSchema: z.object({
      filePaths: filePathsSchema.describe(
        `Path or array of up to ${MAX_FILE_PATHS} paths to source files relative to the repo root (example: 'crates/bharatcode-core/src/agents/agent.rs' or ['ui/text/src/tui.js', 'crates/bharatcode-server/src/main.rs'])`,
      ),
      startLine: startLineSchema
        .optional()
        .describe("Starting line number (0-indexed, default 0)"),
      lineCount: z
        .number()
        .int()
        .min(1)
        .max(MAX_CODE_LINES)
        .optional()
        .describe(
          `Number of lines to show (default 200, max ${MAX_CODE_LINES}). Use smaller values for focused reading, larger for overview.`,
        ),
    }),
    execute: async ({ filePaths, startLine = 0, lineCount = 200 }) => {
      try {
        const result = viewCodebaseFiles(filePaths, startLine, lineCount);
        const count = Array.isArray(filePaths) ? filePaths.length : 1;
        logger.verbose(`Viewed ${count} codebase file(s)`);
        return result;
      } catch (error) {
        const errorMsg =
          error instanceof Error ? error.message : "Unknown error";
        logger.error(`Error viewing codebase: ${errorMsg}`);
        return `Error viewing codebase: ${errorMsg}`;
      }
    },
  }),
  list_codebase_files: tool({
    description:
      "List files and directories in a codebase directory. Use this to explore the project structure before viewing specific files. Only works within ui/ and crates/.",
    inputSchema: z.object({
      directory: z
        .string()
        .min(1)
        .max(MAX_PATH_LENGTH)
        .describe(
          "Directory path relative to repo root, inside ui/ or crates/ (example: 'crates/bharatcode-core/src', 'ui/text/src')",
        ),
    }),
    execute: async ({ directory }) => {
      try {
        const entries = listCodebaseFiles(directory);

        if (entries.length === 0) {
          return `Directory "${directory}" is empty.`;
        }

        return entries
          .map((e) => `${e.isDirectory ? "[dir] " : "      "}${e.filePath}`)
          .join("\n");
      } catch (error) {
        const errorMsg =
          error instanceof Error ? error.message : "Unknown error";
        logger.error(`Error listing codebase files: ${errorMsg}`);
        return `Error listing files: ${errorMsg}`;
      }
    },
  }),
};
