import {
  ChannelType,
  Client,
  Events,
  Message,
  type OmitPartialGroupDMChannel,
} from "discord.js";
import { answerQuestion } from "../utils/ai";
import { buildServerContext } from "../utils/discord/server-context";
import { logger } from "../utils/logger";

const MAX_THREAD_NAME_LENGTH = 100;
const HISTORY_LIMIT = 10;
const MAX_CONCURRENT_QUESTIONS = 4;
const USER_COOLDOWN_MS = 30_000;
let activeQuestions = 0;
const lastQuestionAt = new Map<string, number>();

function acquireQuestionSlot(
  guildId: string | null,
  userId: string,
): (() => void) | null {
  const now = Date.now();
  const key = `${guildId ?? "dm"}:${userId}`;
  const previous = lastQuestionAt.get(key) ?? 0;
  if (
    now - previous < USER_COOLDOWN_MS ||
    activeQuestions >= MAX_CONCURRENT_QUESTIONS
  ) {
    return null;
  }
  lastQuestionAt.set(key, now);
  activeQuestions++;
  return () => {
    activeQuestions = Math.max(0, activeQuestions - 1);
  };
}

function serverContextFor(
  message: OmitPartialGroupDMChannel<Message<boolean>>,
): Promise<string> {
  return message.guild
    ? buildServerContext(message.guild)
    : Promise.resolve("");
}

/** Discord thread names are capped at 100 characters and cannot be empty. */
export function buildThreadName(content: string, fallback: string): string {
  const chars = Array.from(content.trim().replace(/\s+/g, " "));

  if (chars.length === 0) {
    return Array.from(`Question from ${fallback}`)
      .slice(0, MAX_THREAD_NAME_LENGTH)
      .join("");
  }

  if (chars.length <= MAX_THREAD_NAME_LENGTH) {
    return chars.join("");
  }

  return chars.slice(0, MAX_THREAD_NAME_LENGTH - 3).join("") + "...";
}

export default {
  event: Events.MessageCreate,
  handler: async (
    _client: Client,
    message: OmitPartialGroupDMChannel<Message<boolean>>,
  ) => {
    if (message.author.bot) return;

    const questionChannelId = process.env.QUESTION_CHANNEL_ID;

    if (!questionChannelId) {
      logger.verbose("QUESTION_CHANNEL_ID is not configured; ignoring message");
      return;
    }

    // Handle messages in threads
    if (message.channel.isThread()) {
      const parentChannelId =
        message.channel.parent?.id ?? message.channel.parentId;

      if (!parentChannelId || parentChannelId !== questionChannelId) {
        logger.verbose(
          `Ignoring thread message from ${message.author.username} (thread not in question channel)`,
        );
        return;
      }

      try {
        if (!message.content.trim()) {
          await message.reply("Please include a text question.");
          return;
        }

        // Check if the bot was mentioned or replied to
        const isMentioned = message.mentions.has(message.client.user?.id || "");

        let isReplyToBot = false;
        if (message.reference?.messageId) {
          isReplyToBot = await message.channel.messages
            .fetch(message.reference.messageId)
            .then((msg) => msg.author.bot)
            .catch(() => false);
        }

        if (!isMentioned && !isReplyToBot) {
          logger.verbose(
            `Ignoring thread message from ${message.author.username} (not mentioned or replied to)`,
          );
          return;
        }

        const releaseSlot = acquireQuestionSlot(
          message.guildId,
          message.author.id,
        );
        if (!releaseSlot) {
          await message.reply(
            "Please wait before asking another question; the assistant is busy.",
          );
          return;
        }

        try {
          const messages = await message.channel.messages.fetch({
            limit: HISTORY_LIMIT,
          });
          const sortedMessages = Array.from(messages.values())
            .reverse()
            .map((msg) => ({
              author:
                msg.author?.displayName || msg.author?.username || "Unknown",
              content: msg.content,
              isBot: msg.author.bot,
            }));

          await answerQuestion({
            question: message.content,
            thread: message.channel,
            userId: message.author.id,
            messageHistory: sortedMessages,
            serverContext: await serverContextFor(message),
          });
        } finally {
          releaseSlot();
        }

        logger.verbose(
          `Answered follow-up question for ${message.author.username} in thread`,
        );
      } catch (error) {
        logger.error(`Error handling thread message: ${error}`);
      }
      return;
    }

    // Handle initial questions in the question channel
    if (
      message.channelId === questionChannelId &&
      message.channel.type === ChannelType.GuildText
    ) {
      try {
        if (!message.content.trim()) {
          await message.reply("Please include a text question.");
          return;
        }
        const releaseSlot = acquireQuestionSlot(
          message.guildId,
          message.author.id,
        );
        if (!releaseSlot) {
          await message.reply(
            "Please wait before asking another question; the assistant is busy.",
          );
          return;
        }

        try {
          const thread = await message.startThread({
            name: buildThreadName(message.content, message.author.username),
            autoArchiveDuration: 60,
          });

          // Send status message that will be updated as tools are called
          const statusMessage = await thread.send("Just a sec...");

          await answerQuestion({
            question: message.content,
            thread,
            userId: message.author.id,
            statusMessage,
            serverContext: await serverContextFor(message),
          });

        } finally {
          releaseSlot();
        }

        logger.verbose(`Answered question for ${message.author.username}`);
      } catch (error) {
        logger.error(`Error handling question: ${error}`);
      }
    }
  },
};
