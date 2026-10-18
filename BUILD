package(default_visibility = ["//visibility:public"])

exports_files([
    "MODULE.bazel",
    ".gitignore",
    "biome.json",
    "ultracite.json",
])

# Buildifier from @buildifier_prebuilt, exposed for Bazel formatter targets.
alias(
    name = "buildifier",
    actual = "@buildifier_prebuilt//:buildifier",
)

filegroup(
    name = "config_project",
    srcs = ["config/project.json"],
)

# Source inputs needed when Cargo is executed from a Bazel test runfiles tree.
filegroup(
    name = "cargo_workspace",
    srcs = [
        "//:Cargo.lock",
        "//:Cargo.toml",
        "//:config_project",
        "//:.cargo/config.toml",
        "//:rust-toolchain.toml",
        "//crates:all_crates",
        "//crates/code-plugins:bundled_plugins",
        "//examples:srcs",
        "//sdks/rust:rust_sdk_srcs",
        "//tests:all_test_crates",
        "//tools:tool_sources",
    ],
)

# Aggregate Bazel entrypoints replace the former Makefile facade.
filegroup(
    name = "build",
    srcs = [
        "//crates/code-api:code-api",
        "//cli:code",
        "//crates/code-server:code-server",
        "//tests/mock-gateway:mock-gateway",
        "//tools/benchmark:benchmark",
        "//tools/cache:cache",
        "//tools/codegen:codegen",
        "//tools/coverage:coverage",
        "//tools/doctor:doctor",
        "//tools/fuzz:fuzz",
        "//tools/generators:generators",
        "//tools/remote:remote",
        "//tools/telemetry:telemetry",
        "//tools/version:version",
        "//tools/workspace:workspace",
    ],
)

test_suite(
    name = "test",
    tests = [
        "//crates/code-api:test",
        "//cli:test",
        "//crates/code-commands:test",
        "//crates/code-core:test",
        "//crates/code-embeddings:test",
        "//crates/code-events:test",
        "//crates/code-harness:test",
        "//crates/code-integrations:test",
        "//crates/code-memory:test",
        "//crates/code-orchestrator:test",
        "//crates/code-plugins:test",
        "//crates/code-providers:test",
        "//crates/code-repo:test",
        "//crates/code-runtime:test",
        "//crates/code-sandbox:test",
        "//crates/code-server:test",
        "//crates/code-telemetry:test",
        "//crates/code-tools:test",
        "//crates/code-training:test",
        "//crates/code-webhooks:test",
        "//examples:core_config_example",
        "//sdks/rust:test",
        "//sdks/typescript:test",
        "//extensions/frontal-code-slack:test",
        "//tests/mock-gateway:test",
    ],
)

test_suite(
    name = "lint",
    tests = [
        "//crates/code-api:lint",
        "//cli:lint",
        "//crates/code-commands:lint",
        "//crates/code-core:lint",
        "//crates/code-embeddings:lint",
        "//crates/code-events:lint",
        "//crates/code-harness:lint",
        "//crates/code-integrations:lint",
        "//crates/code-memory:lint",
        "//crates/code-orchestrator:lint",
        "//crates/code-plugins:lint",
        "//crates/code-providers:lint",
        "//crates/code-repo:lint",
        "//crates/code-runtime:lint",
        "//crates/code-sandbox:lint",
        "//crates/code-server:lint",
        "//crates/code-telemetry:lint",
        "//crates/code-tools:lint",
        "//crates/code-training:lint",
        "//crates/code-webhooks:lint",
        "//sdks/rust:lint",
        "//sdks/typescript:lint",
        "//extensions/frontal-code-ide:lint",
        "//extensions/frontal-code-slack:lint",
        "//tests/mock-gateway:lint",
    ],
)

test_suite(
    name = "fmt_check",
    tests = [
        "//crates/code-api:fmt",
        "//cli:fmt",
        "//crates/code-commands:fmt",
        "//crates/code-core:fmt",
        "//crates/code-embeddings:fmt",
        "//crates/code-events:fmt",
        "//crates/code-harness:fmt",
        "//crates/code-integrations:fmt",
        "//crates/code-memory:fmt",
        "//crates/code-orchestrator:fmt",
        "//crates/code-plugins:fmt",
        "//crates/code-providers:fmt",
        "//crates/code-repo:fmt",
        "//crates/code-runtime:fmt",
        "//crates/code-sandbox:fmt",
        "//crates/code-server:fmt",
        "//crates/code-telemetry:fmt",
        "//crates/code-tools:fmt",
        "//crates/code-training:fmt",
        "//crates/code-webhooks:fmt",
        "//sdks/rust:fmt",
        "//tests/mock-gateway:fmt",
    ],
)

test_suite(
    name = "ci",
    tests = [
        ":test",
        ":lint",
        ":fmt_check",
        "//scripts:third_party_check",
    ],
)

sh_binary(
    name = "fmt",
    srcs = ["//tools:format.sh"],
    data = [":cargo_workspace"],
)

sh_binary(name = "bootstrap", srcs = ["//scripts:bootstrap.sh"])
sh_binary(name = "clean", srcs = ["//scripts:clean.sh"])
sh_binary(name = "tidy", srcs = ["//scripts:tidy.sh"])
alias(name = "cli", actual = "//scripts:cli")
sh_binary(name = "changeset", srcs = ["//scripts:changeset.sh"])
sh_binary(name = "renovate", srcs = ["//scripts:renovate.sh"])

alias(name = "coverage", actual = "//tools/coverage:coverage")
alias(name = "doctor", actual = "//tools/doctor:doctor")
alias(name = "cache", actual = "//tools/cache:cache")
alias(name = "workspace", actual = "//tools/workspace:workspace")
alias(name = "version", actual = "//tools/version:version")
alias(name = "benchmark", actual = "//tools/benchmark:benchmark")
alias(name = "telemetry", actual = "//tools/telemetry:telemetry")
alias(name = "codegen", actual = "//tools/codegen:codegen")
alias(name = "generators", actual = "//tools/generators:generators")
alias(name = "remote", actual = "//tools/remote:remote")
alias(name = "fuzz", actual = "//tools/fuzz:fuzz")
