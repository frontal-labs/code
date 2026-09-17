package(default_visibility = ["//visibility:public"])

exports_files(["MODULE.bazel"])

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
        "//crates/api:frontal-code-api",
        "//crates/cli:frontal-code",
        "//crates/server:frontal-code-server",
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
        "//crates/api:test",
        "//crates/cli:test",
        "//crates/commands:test",
        "//crates/core:test",
        "//crates/embeddings:test",
        "//crates/events:test",
        "//crates/harness:test",
        "//crates/integrations:test",
        "//crates/memory:test",
        "//crates/orchestrator:test",
        "//crates/plugins:test",
        "//crates/providers:test",
        "//crates/repo:test",
        "//crates/runtime:test",
        "//crates/sandbox:test",
        "//crates/server:test",
        "//crates/telemetry:test",
        "//crates/tools:test",
        "//crates/training:test",
        "//crates/webhooks:test",
        "//examples:core_config_example",
        "//sdks/rust:test",
        "//sdks/typescript:test",
        "//cli:test",
        "//extensions/frontal-code-slack:test",
        "//tests/mock-gateway:test",
    ],
)

test_suite(
    name = "lint",
    tests = [
        "//crates/api:lint",
        "//crates/cli:lint",
        "//crates/commands:lint",
        "//crates/core:lint",
        "//crates/embeddings:lint",
        "//crates/events:lint",
        "//crates/harness:lint",
        "//crates/integrations:lint",
        "//crates/memory:lint",
        "//crates/orchestrator:lint",
        "//crates/plugins:lint",
        "//crates/providers:lint",
        "//crates/repo:lint",
        "//crates/runtime:lint",
        "//crates/sandbox:lint",
        "//crates/server:lint",
        "//crates/telemetry:lint",
        "//crates/tools:lint",
        "//crates/training:lint",
        "//crates/webhooks:lint",
        "//sdks/rust:lint",
        "//sdks/typescript:lint",
        "//cli:lint",
        "//extensions/frontal-code-ide:lint",
        "//extensions/frontal-code-slack:lint",
        "//tests/mock-gateway:lint",
    ],
)

test_suite(
    name = "fmt_check",
    tests = [
        "//crates/api:fmt",
        "//crates/cli:fmt",
        "//crates/commands:fmt",
        "//crates/core:fmt",
        "//crates/embeddings:fmt",
        "//crates/events:fmt",
        "//crates/harness:fmt",
        "//crates/integrations:fmt",
        "//crates/memory:fmt",
        "//crates/orchestrator:fmt",
        "//crates/plugins:fmt",
        "//crates/providers:fmt",
        "//crates/repo:fmt",
        "//crates/runtime:fmt",
        "//crates/sandbox:fmt",
        "//crates/server:fmt",
        "//crates/telemetry:fmt",
        "//crates/tools:fmt",
        "//crates/training:fmt",
        "//crates/webhooks:fmt",
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
