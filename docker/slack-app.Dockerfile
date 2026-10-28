FROM oven/bun:1-alpine@sha256:7ed9f74c326d1c260abe247ac423ccbf5ac92af62bb442d515d1f92f21e8ea9b AS builder

WORKDIR /app

COPY extensions/code-slack/package*.json ./
COPY extensions/code-slack/bun.lock ./

RUN bun install --frozen-lockfile

COPY extensions/code-slack/ ./

RUN bun build --target node --external pg-native src/main.ts src/migrate.ts --outdir dist

# ocp-sdk requires Node >= 22.18.
FROM node:22-alpine@sha256:f598378b5240225e6beab68fa9f356db1fb8efe55173e6d4d8153113bb8f333c

RUN apk add --no-cache dumb-init

RUN addgroup -g 1001 -S frontal-code \
    && adduser -S frontal-code -u 1001 -G frontal-code

WORKDIR /app

COPY extensions/code-slack/package*.json ./
ENV NODE_ENV=production
RUN npm ci --omit=dev && npm cache clean --force

COPY --from=builder /app/dist ./dist

RUN mkdir -p /tmp && chown -R frontal-code:frontal-code /app /tmp

USER frontal-code

ENTRYPOINT ["dumb-init", "--"]
CMD ["node", "dist/main.js"]
