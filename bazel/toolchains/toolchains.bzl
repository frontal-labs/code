# Toolchain resolution helpers built on select().
#
# These helpers keep toolchain selection declarative and centralized. Actual
# toolchains are registered in MODULE.bazel; this file only provides select()
# scaffolding so packages can opt into a toolchain type.

def toolchain_for(conditions, fallback = None, no_match_error = None):
    """Return a select() that resolves a toolchain label by constraint.

    Args:
        conditions: A dict mapping constraint_value label -> toolchain label.
            Bazel picks the toolchain whose constraint matches the target
            platform. Example:
                {
                    "//bazel/constraints:arch_arm64": "//toolchains/rust:arm64",
                    "//bazel/constraints:arch_x86_64": "//toolchains/rust:x86_64",
                }
        fallback: An optional default toolchain label used when no constraint
            matches. Mutually exclusive with no_match_error.
        no_match_error: An error message surfaced when no constraint matches.
            Mutually exclusive with fallback. Defaults to a generic message.

    Returns:
        A select() expression keyed on constraint values.
    """
    if fallback is not None and no_match_error is not None:
        fail("toolchain_for: specify either fallback or no_match_error, not both")
    if fallback is not None:
        return select(conditions, default = fallback)
    return select(conditions, no_match_error = no_match_error or "no matching toolchain for the target platform")
