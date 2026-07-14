import { stepCountIs, streamText } from "ai";
import type { Message, ThreadChannel } from "discord.js";
import { model } from "../../clients/ai";
import { logger } from "../logger";
import { chunkMarkdown } from "./chunk-markdown";
import { MAX_STEPS, buildSystemPrompt } from "./system-prompt";
import { ToolTracker } from "./tool-tracker";
import { aiTools } from "./tools";

export interface MessageHistoryItem {
  author: string;
  content: string;
  isBot: boolean;
}

export interface AnswerQuestionOptions {
  question: string;
  thread: ThreadChannel;
  userId: string;
  messageHistory?: MessageHistoryItem[];
  statusMessage?: Message;
  serverContext?: string;
  abortSignal?: AbortSignal;
}

export const MAX_QUESTION_LENGTH = 4000;
const MAX_HISTORY_MESSAGES = 10;
const MAX_HISTORY_ITEM_LENGTH = 1000;
const MAX_AUTHOR_LENGTH = 80;
const MAX_TOOL_OUTPUT_SCAN = 20_000;
const MAX_REPLY_CHUNKS = 5;

const DEFAULT_TIMEOUT_MS = 120_000;
const MIN_TIMEOUT_MS = 10_000;
const MAX_TIMEOUT_MS = 300_000;

const TIMEOUT_MESSAGE =
  "Sorry, that took too long to research. Try narrowing your question and asking again.";
const ERROR_MESSAGE =
  "Sorry, I encountered an error while researching your question. Please try again.";

function requestTimeoutMs(): number {
  const configured = Number(process.env.AI_REQUEST_TIMEOUT_MS);
  if (!Number.isFinite(configured) || configured <= 0) return DEFAULT_TIMEOUT_MS;
  return Math.min(Math.max(configured, MIN_TIMEOUT_MS), MAX_TIMEOUT_MS);
}

function truncate(text: string, maxLength: number): string {
  const chars = Array.from(text);
  if (chars.length <= maxLength) return chars.join("");
  return chars.slice(0, maxLength).join("") + "…";
}

function buildPrompt(
  question: string,
  messageHistory: MessageHistoryItem[] | undefined,
): string {
  const trimmedQuestion = truncate(question, MAX_QUESTION_LENGTH);

  if (!messageHistory || messageHistory.length === 0) {
    return trimmedQuestion;
  }

  const recent = messageHistory.slice(-MAX_HISTORY_MESSAGES);
  const latest = recent[recent.length - 1];

  const historyContext = recent
    .slice(0, -1)
    .map(
      (msg) =>
        `${truncate(msg.author, MAX_AUTHOR_LENGTH)}: ${truncate(msg.content, MAX_HISTORY_ITEM_LENGTH)}`,
    )
    .join("\n");

  if (!historyContext) {
    return trimmedQuestion;
  }

  return `# Previous conversation\n${historyContext}\n\n# New message\n${truncate(latest.author, MAX_AUTHOR_LENGTH)}: ${trimmedQuestion}`;
}

function countMatches(output: unknown, pattern: RegExp): number {
  const text = String(output).slice(0, MAX_TOOL_OUTPUT_SCAN);
  return (text.match(pattern) ?? []).length;
}

function toPathArray(input: unknown): string[] {
  const filePaths = (input as { filePaths?: string | string[] } | undefined)
    ?.filePaths;
  if (Array.isArray(filePaths)) return filePaths;
  return filePaths ? [filePaths] : [];
}

function isAbort(error: unknown, signal: AbortSignal): boolean {
  if (signal.aborted) return true;
  return error instanceof Error && error.name === "AbortError";
}

export async function answerQuestion({
  question,
  thread,
  userId,
  messageHistory,
  statusMessage,
  serverContext,
  abortSignal,
}: AnswerQuestionOptions): Promise<void> {
  const timeout = AbortSignal.timeout(requestTimeoutMs());
  const signal = abortSignal
    ? AbortSignal.any([abortSignal, timeout])
    : timeout;

  try {
    const prompt = buildPrompt(question, messageHistory);
    const tracker = new ToolTracker();

    const result = streamText({
      model,
      system: buildSystemPrompt(serverContext),
      prompt,
      tools: aiTools,
      stopWhen: stepCountIs(MAX_STEPS),
      abortSignal: signal,
    });

    for await (const event of result.fullStream) {
      if (event.type === "tool-call") {
        await updateStatus(statusMessage, event.toolName, event.input);
      } else if (event.type === "tool-result") {
        recordToolResult(tracker, event.toolName, event.input, event.output);
      } else if (event.type === "error") {
        throw event.error;
      } else if (event.type === "abort") {
        throw signal.reason ?? new DOMException("Request aborted", "AbortError");
      }
    }

    if (statusMessage) {
      try {
        await statusMessage.edit(tracker.getSummary() || "Just a sec...");
      } catch (error) {
        logger.verbose("Failed to update final status message:", error);
      }
    }

    const chunks = chunkMarkdown(await result.text);
    if (chunks.length === 0) {
      await thread.send(ERROR_MESSAGE);
      return;
    }
    if (chunks.length > MAX_REPLY_CHUNKS) {
      logger.warn(
        `Answer for user ${userId} exceeded ${MAX_REPLY_CHUNKS} chunks; truncating`,
      );
    }

    const outgoing = chunks.slice(0, MAX_REPLY_CHUNKS);
    if (chunks.length > MAX_REPLY_CHUNKS) {
      outgoing[MAX_REPLY_CHUNKS - 1] =
        outgoing[MAX_REPLY_CHUNKS - 1].slice(0, 1940) +
        "\n\n_Response truncated; ask a narrower follow-up._";
    }
    for (const chunk of outgoing) {
      await thread.send(chunk);
    }

    const { totalTokens } = await result.usage;
    logger.verbose(
      `Answered question for user ${userId}, tokens: ${totalTokens}`,
    );
  } catch (error) {
    const aborted = isAbort(error, signal);

    if (aborted) {
      logger.warn(`Question from user ${userId} was cancelled or timed out`);
    } else {
      logger.error("Failed to answer question:", error);
    }

    await thread
      .send(aborted ? TIMEOUT_MESSAGE : ERROR_MESSAGE)
      .catch((sendError) =>
        logger.error("Failed to send error message:", sendError),
      );

    throw error;
  }
}

async function updateStatus(
  statusMessage: Message | undefined,
  toolName: string,
  input: unknown,
): Promise<void> {
  if (!statusMessage) return;

  const status = statusText(toolName, input);
  if (!status) return;

  try {
    await statusMessage.edit(status);
  } catch (error) {
    logger.verbose("Failed to update status message:", error);
  }
}

function statusText(toolName: string, input: unknown): string | undefined {
  switch (toolName) {
    case "search_docs":
      return "Searching the docs...";
    case "search_codebase":
      return "Searching the codebase...";
    case "list_codebase_files":
      return "Exploring project structure...";
    case "view_docs": {
      const count = toPathArray(input).length;
      return `Viewing ${count} ${count === 1 ? "page" : "pages"}...`;
    }
    case "view_codebase": {
      const count = toPathArray(input).length;
      return `Reading ${count} source ${count === 1 ? "file" : "files"}...`;
    }
    default:
      return undefined;
  }
}

function recordToolResult(
  tracker: ToolTracker,
  toolName: string,
  input: unknown,
  output: unknown,
): void {
  switch (toolName) {
    case "search_docs": {
      const count = countMatches(output, /\*\*[^*]+\*\*/g);
      tracker.recordSearchCall(
        Array.from({ length: count }, (_, i) => `result_${i}`),
      );
      break;
    }
    case "search_codebase":
      tracker.recordCodeSearchCall(countMatches(output, /\*\*[^*]+:\d+\*\*/g));
      break;
    case "list_codebase_files":
      tracker.recordListDir();
      break;
    case "view_docs": {
      const paths = toPathArray(input);
      if (paths.length > 0) tracker.recordViewCall(paths);
      break;
    }
    case "view_codebase": {
      const paths = toPathArray(input);
      if (paths.length > 0) tracker.recordCodeViewCall(paths);
      break;
    }
  }
}
