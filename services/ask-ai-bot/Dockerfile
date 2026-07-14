FROM oven/bun:1@sha256:e10577f0db68676a7024391c6e5cb4b879ebd17188ab750cf10024a6d700e5c4 AS base
WORKDIR /app

# Install dependencies
FROM base AS deps
COPY services/ask-ai-bot/package.json services/ask-ai-bot/bun.lock ./
RUN bun install --frozen-lockfile

# Build stage
FROM base AS build
COPY --from=deps /app/node_modules ./node_modules
COPY services/ask-ai-bot/ ./
RUN bun run build

FROM build AS test
RUN bun test

# Production stage
FROM base AS production
ENV NODE_ENV=production
ENV DOCS_PATH=/app/docs
ENV CODEBASE_PATH=/app/codebase

COPY --from=build /app/dist ./dist
COPY --from=build /app/node_modules ./node_modules
COPY --from=build /app/package.json ./

# Ship the repository's maintained Markdown as the bot's local documentation corpus.
RUN mkdir -p /app/docs
COPY README.md CONTRIBUTING.md ./docs/

# Copy codebase source for code browsing
COPY ui/ ./codebase/ui/
COPY crates ./codebase/crates

# Empty index.ts for discraft start to detect
RUN touch index.ts

CMD ["bun", "run", "start"]
