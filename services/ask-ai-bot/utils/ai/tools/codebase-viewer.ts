import fs from "fs";
import path from "path";

const GITHUB_BASE_URL =
  "https://github.com/arbazkhan971/bharatcode-cli/blob/main";
const MAX_FILE_BYTES = 512 * 1024;

function getCodebaseDir(): string {
  return process.env.CODEBASE_PATH || path.join(process.cwd(), "../..");
}

function generateGitHubUrl(filePath: string, startLine?: number): string {
  const url = `${GITHUB_BASE_URL}/${filePath}`;
  if (startLine && startLine > 0) {
    return `${url}#L${startLine}`;
  }
  return url;
}

function isWithin(root: string, target: string): boolean {
  return target === root || target.startsWith(root + path.sep);
}

function resolveAllowedFile(filePath: string): string {
  const baseDir = fs.realpathSync(path.resolve(getCodebaseDir()));
  const lexicalPath = path.resolve(baseDir, filePath);
  if (!isWithin(baseDir, lexicalPath)) {
    throw new Error("Invalid file path - directory traversal not allowed");
  }

  const allowedRoots = ["ui", "crates"]
    .map((name) => path.join(baseDir, name))
    .filter(
      (candidate) =>
        fs.existsSync(candidate) && !fs.lstatSync(candidate).isSymbolicLink(),
    )
    .map((candidate) => fs.realpathSync(candidate));
  const stat = fs.lstatSync(lexicalPath);
  if (stat.isSymbolicLink() || !stat.isFile()) {
    throw new Error(`Path is not a regular source file: ${filePath}`);
  }
  const realPath = fs.realpathSync(lexicalPath);
  if (!allowedRoots.some((root) => isWithin(root, realPath))) {
    throw new Error("Invalid file path - only ui/ and crates/ source files are readable");
  }
  if (stat.size > MAX_FILE_BYTES) {
    throw new Error(`File is too large to view safely: ${filePath}`);
  }
  return realPath;
}

function getCodeChunk(
  filePath: string,
  startLine: number = 0,
  lineCount: number = 200,
): {
  filePath: string;
  content: string;
  totalLines: number;
  githubUrl: string;
} {
  const fullPath = resolveAllowedFile(filePath);

  const content = fs.readFileSync(fullPath, "utf-8");
  const lines = content.split("\n");
  const totalLines = lines.length;

  const actualStart = Math.max(0, Math.min(startLine, lines.length - 1));
  const actualEnd = Math.min(actualStart + lineCount, lines.length);
  const chunkLines = lines.slice(actualStart, actualEnd);

  const numberedContent = chunkLines
    .map((line, i) => `${actualStart + i + 1}: ${line}`)
    .join("\n");

  return {
    filePath,
    content: numberedContent,
    totalLines,
    githubUrl: generateGitHubUrl(
      filePath,
      actualStart > 0 ? actualStart + 1 : undefined,
    ),
  };
}

export function viewCodebaseFiles(
  filePaths: string | string[],
  startLine: number = 0,
  lineCount: number = 200,
): string {
  const paths = Array.isArray(filePaths) ? filePaths : [filePaths];

  const results = paths.map((filePath) => {
    const chunk = getCodeChunk(filePath, startLine, lineCount);
    const ext = path.extname(filePath).slice(1) || "text";
    const lineInfo =
      startLine > 0
        ? ` (lines ${startLine + 1}-${Math.min(startLine + lineCount, chunk.totalLines)} of ${chunk.totalLines})`
        : ` (${chunk.totalLines} lines total)`;

    return `**${chunk.filePath}**${lineInfo}\nGitHub: <${chunk.githubUrl}>\n\`\`\`${ext}\n${chunk.content}\n\`\`\``;
  });

  return results.join("\n\n---\n\n");
}
