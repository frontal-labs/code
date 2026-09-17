FROM oven/bun:1-alpine AS builder

WORKDIR /app

COPY extensions/frontal-code-slack/package*.json ./
COPY extensions/frontal-code-slack/bun.lock ./

RUN bun install --frozen-lockfile

COPY extensions/frontal-code-slack/ ./

RUN bun build --target node src/index.ts --outdir dist

FROM node:20-alpine

RUN apk add --no-cache dumb-init

RUN addgroup -g 1001 -S frontal-code \
    && adduser -S frontal-code -u 1001 -G frontal-code

WORKDIR /app

COPY extensions/frontal-code-slack/package*.json ./
ENV NODE_ENV=production
RUN npm ci --omit=dev && npm cache clean --force

COPY --from=builder /app/dist ./dist

RUN mkdir -p /tmp && chown -R frontal-code:frontal-code /app /tmp

USER frontal-code

ENTRYPOINT ["dumb-init", "--"]
CMD ["node", "dist/index.js"]
