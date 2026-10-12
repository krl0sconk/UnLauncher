# Build log fixtures

`synthetic-failed-build.log` is **SYNTHETIC**: it was written by hand to
exercise the progress parser and the known-errors table. It imitates the shape
of `Setup.sh` (GitDependencies) and UnrealBuildTool output but it is not a
recording of a real build, and its error lines are generic messages (kernel,
clang, glibc), not verified Unreal output.

Real logs must still be recorded (a manual task): build
UE 5.7 from source on Linux with UnLauncher, take the log it writes
(`BuildOptions::log_path`), trim it to the relevant parts (start of each step,
a few hundred `[n/total]` lines, the end) and add it here as
`<version>-<distro>-success.log`. Do the same for every real failure seen, and
add or fix the matching entry in `data/known_errors.toml`. Remove any personal
paths or user names before committing.
