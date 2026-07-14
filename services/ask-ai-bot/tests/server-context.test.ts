import { describe, expect, it } from "bun:test";
import { ChannelType, PermissionFlagsBits } from "discord.js";
import {
  __testables,
  buildServerContext,
} from "../utils/discord/server-context";

const { isPublicChannel, truncateTopic, MAX_LISTED_CHANNELS } = __testables;

const GUILD_ID = "guild-1";

const guild = {
  id: GUILD_ID,
  roles: { everyone: { id: GUILD_ID } },
} as never;

interface FakeChannelOptions {
  id?: string;
  name?: string;
  topic?: string;
  nsfw?: boolean;
  canView?: boolean;
  parentCanView?: boolean;
  hasParent?: boolean;
  permissionsFor?: () => unknown;
  position?: number;
}

function fakeChannel(options: FakeChannelOptions = {}) {
  const {
    id = "chan-1",
    name = "general",
    topic,
    nsfw = false,
    canView = true,
    parentCanView = true,
    hasParent = false,
    permissionsFor,
    position = 0,
  } = options;

  const permissions = (allowed: boolean) => ({
    has: (flag: bigint) => flag === PermissionFlagsBits.ViewChannel && allowed,
  });

  return {
    id,
    name,
    topic,
    nsfw,
    position,
    type: ChannelType.GuildText,
    parent: hasParent
      ? { id: "cat-1", permissionsFor: () => permissions(parentCanView) }
      : null,
    permissionsFor: permissionsFor ?? (() => permissions(canView)),
  } as never;
}

describe("isPublicChannel", () => {
  it("includes a channel @everyone can view", () => {
    expect(isPublicChannel(fakeChannel(), guild)).toBe(true);
  });

  it("excludes a channel that denies ViewChannel to @everyone", () => {
    expect(isPublicChannel(fakeChannel({ canView: false }), guild)).toBe(false);
  });

  it("excludes a channel that inherits a private category", () => {
    const channel = fakeChannel({
      canView: true,
      hasParent: true,
      parentCanView: false,
    });

    expect(isPublicChannel(channel, guild)).toBe(false);
  });

  it("includes a channel under a public category", () => {
    const channel = fakeChannel({ hasParent: true, parentCanView: true });

    expect(isPublicChannel(channel, guild)).toBe(true);
  });

  it("excludes NSFW channels even when publicly viewable", () => {
    expect(isPublicChannel(fakeChannel({ nsfw: true }), guild)).toBe(false);
  });

  it("excludes a channel whose permissions cannot be resolved", () => {
    const nullPerms = fakeChannel({ permissionsFor: () => null });
    const throwing = fakeChannel({
      permissionsFor: () => {
        throw new Error("uncached member");
      },
    });

    expect(isPublicChannel(nullPerms, guild)).toBe(false);
    expect(isPublicChannel(throwing, guild)).toBe(false);
  });

  it("excludes every channel when the @everyone role is missing", () => {
    const guildWithoutEveryone = { id: GUILD_ID, roles: {} } as never;

    expect(isPublicChannel(fakeChannel(), guildWithoutEveryone)).toBe(false);
  });
});

describe("truncateTopic", () => {
  it("leaves short topics intact", () => {
    expect(truncateTopic("A public topic")).toBe("A public topic");
  });

  it("caps long topics", () => {
    const truncated = truncateTopic("x".repeat(500));

    expect(Array.from(truncated).length).toBe(200);
    expect(truncated.endsWith("…")).toBe(true);
  });

  it("collapses newlines so one topic cannot forge extra list rows", () => {
    expect(truncateTopic("real\n- ID: 999; Name: fake")).toBe(
      "real - ID: 999; Name: fake",
    );
  });
});

describe("buildServerContext", () => {
  function guildWithChannels(channels: unknown[]) {
    return {
      id: GUILD_ID,
      roles: { everyone: { id: GUILD_ID } },
      channels: { fetch: async () => new Map(channels.map((c, i) => [i, c])) },
    } as never;
  }

  it("never leaks a channel inside a private category", async () => {
    const context = await buildServerContext(
      guildWithChannels([
        fakeChannel({ id: "1", name: "public-chat" }),
        fakeChannel({
          id: "2",
          name: "staff-only",
          topic: "internal planning",
          hasParent: true,
          parentCanView: false,
        }),
        fakeChannel({ id: "3", name: "locked", canView: false }),
        fakeChannel({ id: "4", name: "adults", nsfw: true }),
      ]),
    );

    expect(context).toContain("public-chat");
    expect(context).not.toContain("staff-only");
    expect(context).not.toContain("internal planning");
    expect(context).not.toContain("locked");
    expect(context).not.toContain("adults");
  });

  it("returns an empty string when nothing is public", async () => {
    const context = await buildServerContext(
      guildWithChannels([fakeChannel({ canView: false })]),
    );

    expect(context).toBe("");
  });

  it("caps how many channels are listed", async () => {
    const channels = Array.from({ length: MAX_LISTED_CHANNELS + 25 }, (_, i) =>
      fakeChannel({ id: `c${i}`, name: `chan-${i}`, position: i }),
    );

    const context = await buildServerContext(guildWithChannels(channels));
    const rows = (context.match(/^- ID: /gm) ?? []).length;

    expect(rows).toBe(MAX_LISTED_CHANNELS);
  });

  it("returns an empty string when the channel fetch fails", async () => {
    const guildThatThrows = {
      id: GUILD_ID,
      roles: { everyone: { id: GUILD_ID } },
      channels: {
        fetch: async () => {
          throw new Error("network");
        },
      },
    } as never;

    expect(await buildServerContext(guildThatThrows)).toBe("");
  });
});
