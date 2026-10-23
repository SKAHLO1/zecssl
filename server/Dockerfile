# ZEC SSL read API. Build context: the repository root (it is a pnpm workspace).
FROM node:22-bookworm-slim AS build
WORKDIR /app
RUN corepack enable
COPY package.json pnpm-lock.yaml pnpm-workspace.yaml ./
COPY server/package.json server/
RUN pnpm install --frozen-lockfile --filter @zec-ssl/server
COPY server server
RUN pnpm --filter @zec-ssl/server build \
 && pnpm --filter @zec-ssl/server deploy --prod --legacy /out

FROM node:22-bookworm-slim
WORKDIR /app
COPY --from=build /out .
ENV NODE_ENV=production HOST=0.0.0.0 PORT=4000
EXPOSE 4000
CMD ["node", "dist/index.js"]
