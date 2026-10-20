# Coverage aspect — wired to Bazel's native coverage instrumentation.
#
# Returns `coverage_common.instrumented_files_info(ctx)` so that
# `bazel coverage //...` collects the target's instrumented sources. Apply on
# demand, e.g.:
#   bazel coverage //some:target --aspects=//bazel/aspects:coverage_aspect.bzl%coverage_aspect

def _coverage_aspect_impl(_unused_target, ctx):
    return [coverage_common.instrumented_files_info(ctx)]

coverage_aspect = aspect(
    implementation = _coverage_aspect_impl,
    doc = "Returns coverage_common.instrumented_files_info(ctx) for `bazel coverage`.",
)
