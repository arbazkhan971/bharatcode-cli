import { marked } from "marked";

const MAX_DISCORD_LENGTH = 2000;
const MIN_CHUNK_LENGTH = 32;

interface RawToken {
  raw: string;
  type?: string;
  text?: string;
  lang?: string;
}

/**
 * Chunks markdown text intelligently, respecting markdown structure.
 * Oversized fenced code blocks are re-fenced per chunk so each chunk renders
 * as valid markdown on its own. Splits never land inside a surrogate pair.
 *
 * @param markdown - The markdown text to chunk
 * @param maxLength - Maximum length per chunk (default: 2000 for Discord)
 * @returns Array of markdown chunks, each at most `maxLength` characters
 */
export function chunkMarkdown(
  markdown: string,
  maxLength: number = MAX_DISCORD_LENGTH,
): string[] {
  const limit = Math.max(
    MIN_CHUNK_LENGTH,
    Math.min(maxLength, MAX_DISCORD_LENGTH),
  );

  if (!markdown || !markdown.trim()) {
    return [];
  }

  if (markdown.length <= limit) {
    return [markdown];
  }

  const chunks: string[] = [];
  let currentChunk = "";

  const flush = () => {
    if (currentChunk.trim()) chunks.push(currentChunk);
    currentChunk = "";
  };

  for (const token of lex(markdown)) {
    const tokenText = token.raw;
    if (!tokenText) continue;

    if (currentChunk.length + tokenText.length <= limit) {
      currentChunk += tokenText;
      continue;
    }

    flush();

    if (tokenText.length <= limit) {
      currentChunk = tokenText;
      continue;
    }

    const pieces =
      token.type === "code"
        ? splitCodeBlock(token, limit)
        : containsFence(tokenText)
          ? splitContainerWithFences(tokenText, limit)
        : characterSplit(tokenText, limit);

    for (const piece of pieces.slice(0, -1)) {
      if (piece.trim()) chunks.push(piece);
    }
    currentChunk = pieces[pieces.length - 1] ?? "";
  }

  flush();

  return chunks;
}

function containsFence(text: string): boolean {
  return /^(?:\s*>\s*)*\s*(?:`{3,}|~{3,})/m.test(text);
}

/**
 * Splits blockquotes and list-like containers without leaving a fenced block
 * open across Discord messages. Synthetic closing/opening fences are inserted
 * at boundaries and carry the original quote/indent prefix.
 */
function splitContainerWithFences(text: string, maxLength: number): string[] {
  const chunks: string[] = [];
  let current = "";
  let openFence: { prefix: string; marker: string; info: string } | null = null;

  const pushCurrent = () => {
    if (current.trim()) chunks.push(current);
    current = "";
  };

  const closeLine = () =>
    openFence ? `${openFence.prefix}${openFence.marker}` : "";
  const reopenLine = () =>
    openFence
      ? `${openFence.prefix}${openFence.marker}${openFence.info}\n`
      : "";

  for (const line of text.split("\n")) {
    const separator = current ? "\n" : "";
    const reserve = openFence ? closeLine().length + 1 : 0;

    if (current.length + separator.length + line.length + reserve > maxLength) {
      if (openFence) {
        current += `${separator}${closeLine()}`;
      }
      pushCurrent();
      current = reopenLine();
    }

    if (line.length + current.length > maxLength) {
      for (const piece of characterSplit(line, Math.max(1, maxLength - current.length))) {
        if (current.length + piece.length > maxLength) pushCurrent();
        current += piece;
        if (current.length === maxLength) pushCurrent();
      }
    } else {
      current += `${current ? "\n" : ""}${line}`;
    }

    const fence = line.match(/^((?:\s*>\s*)*\s*)(`{3,}|~{3,})(.*)$/);
    if (!fence) continue;

    if (!openFence) {
      openFence = {
        prefix: fence[1],
        marker: fence[2],
        info: fence[3],
      };
    } else if (
      fence[2][0] === openFence.marker[0] &&
      fence[2].length >= openFence.marker.length &&
      !fence[3].trim()
    ) {
      openFence = null;
    }
  }

  if (openFence && current.trim()) {
    const suffix = `\n${closeLine()}`;
    if (current.length + suffix.length <= maxLength) current += suffix;
  }
  pushCurrent();
  return chunks;
}

function lex(markdown: string): RawToken[] {
  try {
    return marked.lexer(markdown) as unknown as RawToken[];
  } catch {
    return [{ raw: markdown }];
  }
}

/**
 * Splits an oversized fenced code block into chunks that each carry their own
 * opening and closing fence, so no chunk leaks unterminated code formatting.
 */
function splitCodeBlock(token: RawToken, maxLength: number): string[] {
  const body = token.text ?? "";
  const lang = (token.lang ?? "").split(/\s+/)[0] ?? "";
  const fence = "`".repeat(Math.max(3, longestBacktickRun(body) + 1));

  const header = `${fence}${lang}\n`;
  const footer = `\n${fence}`;
  const budget = maxLength - header.length - footer.length;

  if (budget < 1) {
    return characterSplit(token.raw, maxLength);
  }

  return splitBody(body, budget).map((part) => `${header}${part}${footer}`);
}

function longestBacktickRun(text: string): number {
  let longest = 0;
  for (const run of text.match(/`+/g) ?? []) {
    longest = Math.max(longest, run.length);
  }
  return longest;
}

/** Packs lines into groups of at most `budget` characters, hard-splitting long lines. */
function splitBody(body: string, budget: number): string[] {
  const parts: string[] = [];
  let current = "";

  const push = () => {
    if (current) parts.push(current);
    current = "";
  };

  for (const line of body.split("\n")) {
    const candidate = current ? `${current}\n${line}` : line;

    if (candidate.length <= budget) {
      current = candidate;
      continue;
    }

    push();

    if (line.length <= budget) {
      current = line;
      continue;
    }

    const hardSplits = characterSplit(line, budget);
    parts.push(...hardSplits.slice(0, -1));
    current = hardSplits[hardSplits.length - 1] ?? "";
  }

  push();

  return parts.length > 0 ? parts : [""];
}

/**
 * Character-based splitting with word boundary awareness. Split points are
 * pulled back off surrogate pairs and combining marks so no chunk ends with
 * a broken character.
 */
function characterSplit(text: string, maxLength: number): string[] {
  if (text.length <= maxLength) {
    return [text];
  }

  const chunks: string[] = [];
  let remaining = text;

  while (remaining.length > maxLength) {
    let splitIndex = maxLength;

    const whitespaceIndex = lastWhitespaceIndex(remaining, maxLength);
    if (whitespaceIndex > maxLength * 0.8) {
      splitIndex = whitespaceIndex;
    }

    splitIndex = safeBoundary(remaining, splitIndex);

    chunks.push(remaining.slice(0, splitIndex));
    remaining = remaining.slice(splitIndex).trimStart();
  }

  if (remaining) {
    chunks.push(remaining);
  }

  return chunks.length > 0 ? chunks : [""];
}

function lastWhitespaceIndex(text: string, before: number): number {
  const newline = text.lastIndexOf("\n", before);
  const space = text.lastIndexOf(" ", before);
  return Math.max(newline, space);
}

/**
 * Moves an index backwards until it sits on a code point boundary that is not
 * in the middle of a combining sequence. Never returns 0 for a non-empty slice,
 * which would stall the split loop.
 */
function safeBoundary(text: string, index: number): number {
  let i = Math.min(index, text.length);

  while (i > 1 && splitsCharacter(text, i)) {
    i--;
  }

  return i;
}

function splitsCharacter(text: string, index: number): boolean {
  const code = text.charCodeAt(index);
  if (Number.isNaN(code)) return false;

  const isLowSurrogate = code >= 0xdc00 && code <= 0xdfff;
  if (isLowSurrogate) return true;

  return /\p{M}/u.test(text[index] ?? "");
}
