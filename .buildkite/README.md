# Buildkite pipeline

Pipeline defined in `.buildkite/pipeline.yml`. Lint → build → test on every push/PR, then deploy on `main`.

## Secrets and queues
- `default` queue for CI; `deploy` queue for prod.
- Secrets via `buildkite-agent secret get` or secrets plugin; OIDC for cloud creds.
- Enable signed pipelines for prod.
