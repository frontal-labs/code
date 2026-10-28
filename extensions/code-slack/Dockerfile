ARG BUN_IMAGE=oven/bun:1-alpine@sha256:7ed9f74c326d1c260abe247ac423ccbf5ac92af62bb442d515d1f92f21e8ea9b
# ocp-sdk requires Node >= 22.18.
ARG NODE_IMAGE=node:22-alpine@sha256:fe4f6d74e20370acb04ad889077d14d0f64a9c8ea6a42bc60cddf1e844699914

FROM ${BUN_IMAGE} AS builder

# Set working directory
WORKDIR /app

COPY package.json bun.lock bunfig.toml ./

# Install from the committed bun.lock. --frozen-lockfile fails the build if the lock
# is out of date with package.json instead of silently re-resolving dependencies.
RUN if [ -f bun.lock ]; then bun install --frozen-lockfile; else bun install && echo "Note: bun.lock missing; dependencies were resolved without a lockfile"; fi

# Copy source code
COPY . .

# Build the application
RUN bun run build

FROM ${NODE_IMAGE} AS production

# Install dumb-init for proper signal handling
RUN apk add --no-cache dumb-init

RUN addgroup -g 1001 -S frontal-code \
    && adduser -S frontal-code -u 1001 -G frontal-code

# Set working directory
WORKDIR /app

# Copy package files
COPY package*.json ./

ENV NODE_ENV=production

RUN npm ci --omit=dev \
    && npm cache clean --force

# Copy built application from builder stage
COPY --from=builder /app/dist ./dist

RUN mkdir -p /tmp logs \
    && chown -R frontal-code:frontal-code /app /tmp

# Switch to non-root user
USER frontal-code

# Health check
HEALTHCHECK --interval=30s --timeout=10s --start-period=5s --retries=3 \
  CMD node -e "require('http').get('http://127.0.0.1:3000/health', (res) => { process.exit(res.statusCode === 200 ? 0 : 1) }).on('error', () => process.exit(1))"

# Expose port
EXPOSE 3000

# Start the application. Run `node dist/migrate.js` once per deploy to migrate the OCP event store.
ENTRYPOINT ["dumb-init", "--"]
CMD ["node", "dist/main.js"]
