# Syntax:
#   docker build -f sdks/Dockerfile -t ghcr.io/frontal-labs/frontal-code-sdk:latest .
#
# Publishes the Frontal Code TypeScript SDK as a distributable container image on
# the GitHub Container Registry (ghcr.io). The built package is installed globally
# so it can be `require`d/`import`ed from anywhere inside the image.

FROM node:22-slim AS build
WORKDIR /sdks/typescript
COPY sdks/typescript/package.json sdks/typescript/package-lock.json* ./
RUN npm install
COPY sdks/typescript/ ./
RUN npm run build \
    && npm link

FROM node:22-slim
LABEL org.opencontainers.image.source="https://github.com/frontal-labs/frontal-code"
LABEL org.opencontainers.image.description="Frontal Code TypeScript SDK (frontal-code-sdk)"
LABEL org.opencontainers.image.licenses="Apache-2.0"

# The frontal-code CLI is expected to be supplied at runtime (mounted or installed).
ENV PATH="/usr/local/bin:${PATH}"

COPY --from=build /usr/local/lib/node_modules/@frontal-labs/frontal-code-sdk /usr/local/lib/node_modules/@frontal-labs/frontal-code-sdk
RUN ln -s /usr/local/lib/node_modules/@frontal-labs/frontal-code-sdk /usr/local/lib/node_modules/frontal-code-sdk

WORKDIR /work
ENTRYPOINT ["node", "-e", "import('@frontal-labs/frontal-code-sdk').then(m => console.log(Object.keys(m)))"]
