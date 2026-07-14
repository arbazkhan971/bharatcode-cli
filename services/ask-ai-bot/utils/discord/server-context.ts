import type { Guild, GuildBasedChannel, TextChannel } from "discord.js";
import { ChannelType, PermissionFlagsBits } from "discord.js";

const MAX_LISTED_CHANNELS = 100;
const MAX_TOPIC_LENGTH = 200;

function everyoneCanView(ch: GuildBasedChannel, guild: Guild): boolean {
  const everyone = guild.roles.everyone;
  if (!everyone) return false;

  try {
    const permissions = ch.permissionsFor(everyone);
    return permissions?.has(PermissionFlagsBits.ViewChannel) === true;
  } catch {
    return false;
  }
}

/**
 * A channel is only advertised when @everyone can view both the channel and the
 * category it lives under. Discord desyncs child overwrites from their category,
 * so a channel with no explicit deny can still sit inside a private category.
 * Anything we cannot positively resolve as public is treated as private.
 */
function isPublicChannel(ch: TextChannel, guild: Guild): boolean {
  if (ch.nsfw) return false;
  if (!everyoneCanView(ch, guild)) return false;

  const parent = ch.parent;
  if (parent && !everyoneCanView(parent, guild)) return false;

  return true;
}

function truncateTopic(topic: string): string {
  const chars = Array.from(topic.replace(/\s+/g, " ").trim());
  if (chars.length <= MAX_TOPIC_LENGTH) return chars.join("");
  return chars.slice(0, MAX_TOPIC_LENGTH - 1).join("") + "…";
}

export async function buildServerContext(guild: Guild): Promise<string> {
  try {
    const channels = await guild.channels.fetch();

    const textChannels = Array.from(channels.values())
      .filter(
        (ch): ch is TextChannel =>
          ch !== null &&
          ch.type === ChannelType.GuildText &&
          isPublicChannel(ch, guild),
      )
      .sort((a, b) => (a.position ?? 0) - (b.position ?? 0))
      .slice(0, MAX_LISTED_CHANNELS);

    if (textChannels.length === 0) {
      return "";
    }

    const channelList = textChannels
      .map((ch) => {
        const topic = ch.topic ? `Topic: ${truncateTopic(ch.topic)}` : "";
        return `- ID: ${ch.id}; Name: ${ch.name}; ${topic}`;
      })
      .join("\n");

    return `## Server Channels
If a user asks about the server's channels or where to find something, here's the current channel list:
${channelList}

When mentioning a channel, provide the link to the channel rather than using the plain text name. You can link to a channel by using the following format: \`<#channelId>\`.`;
  } catch (error) {
    console.error("Error building server context:", error);
    return "";
  }
}

export const __testables = {
  isPublicChannel,
  truncateTopic,
  MAX_LISTED_CHANNELS,
};
