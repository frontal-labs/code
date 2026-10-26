# Lint aspect — wired to a real linter (buildifier).
#
# For every Starlark file (`.bzl`, `.bazel`, `BUILD`, `WORKSPACE`,
# `MODULE.bazel`) in a target's sources, runs `buildifier --lint=warn
# --mode=check`. The action fails the build when buildifier reports a lint
# warning.
#
# Apply on demand, e.g.:
#   bazel build //some:target --aspects=//bazel/aspects:lint_aspect.bzl%lint_aspect

_STARLARK_EXTS = ["bzl", "bazel"]
_STARLARK_NAMES = ["BUILD", "WORKSPACE", "MODULE.bazel", "BUILD.bazel"]

def _source_file(node):
    """Return the underlying File from a Target or File node."""
    files = getattr(node, "files", None)
    if files:
        flist = files.to_list()
        if flist:
            return flist[0]
    return node

def _source_files(ctx):
    """Resolve the File objects behind a target's `srcs` attribute."""
    srcs = getattr(ctx.rule.attr, "srcs", [])
    return [_source_file(src) for src in srcs]

def _is_starlark(f):
    return f.extension in _STARLARK_EXTS or f.basename in _STARLARK_NAMES

def _lint_aspect_impl(_unused_target, ctx):
    buildifier = ctx.executable._buildifier
    outputs = []
    for src in _source_files(ctx):
        if not _is_starlark(src):
            continue
        out = ctx.actions.declare_file(src.basename + ".linted")
        ctx.actions.run(
            executable = buildifier,
            arguments = ["--lint=warn", "--mode=check", src.path],
            inputs = [src],
            outputs = [out],
            mnemonic = "BuildifierLint",
            progress_message = "Linting %s with buildifier" % src.path,
        )
        outputs.append(out)

    return [OutputGroupInfo(lint = depset(outputs))]

lint_aspect = aspect(
    implementation = _lint_aspect_impl,
    attrs = {
        "_buildifier": attr.label(
            default = "@buildifier_prebuilt//:buildifier",
            executable = True,
            cfg = "exec",
        ),
    },
    doc = "Runs buildifier --lint=warn on Starlark sources of the target.",
)
