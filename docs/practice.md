# Daily coding practice

## Getting started

Open the avatar button in the top-right header and create or select a local profile. Under its calendar and language statistics, click **Enable practice for this profile**. Practice is optional and off by default, including on the initial default profile. Disabling it hides the widget without deleting progress.

The widget runs locally. Choose a language and difficulty, read the problem and examples, then implement the provided function in the embedded solution editor. Keep the starter's function signature: the test harness supplies inputs and checks the return value. You do not need to write a command-line program or parse standard input.

Use **Submit** or **Ctrl+Enter** to compile/run the solution. Results distinguish accepted solutions, wrong answers, compile errors, runtime errors and timeouts. Public examples and hidden test cases both participate in grading. Use **Cancel** to stop a running submission. **Reset** restores the current starter, so copy any work you want to keep first.

The daily problem is selected deterministically from the local calendar date and difficulty. It stays the same across languages; switching languages loads that language's independent starter or saved draft for the same problem. An accepted daily solve updates that profile's streak; repeated solves on the same local date do not create extra streak days. Profile activity and practice use the same machine-local date boundary, not UTC. Drafts, selected language/difficulty and progress belong to the profile's stable `practice/` directory; see [profiles](profiles.md) for storage, opt-in and concurrency details.

## Toolchains

Compilers and interpreters must already be installed and visible on the GUI process's `PATH`. The widget discovers available toolchains and displays a warning for missing ones; selecting a language does not install packages. If you install a compiler while Gtuim is open, restart Gtuim to ensure its inherited environment and toolchain probe see it.

### C practice

- Install a **C11** compiler. The runner searches for `gcc`, then `clang`, then `cc`.
- Select **C**, not **C++**: C submissions use `solution.c` and C11 compilation, while C++ is a separate language/toolchain.
- Implement the exact `solve` signature in the supplied starter. Integer tasks use `long long`, Boolean tasks use `bool`, and array arguments carry an explicit `size_t` length.
- For array-output tasks, write values into the supplied `out` buffer and return the result length. For index-pair tasks, set `*out_a` and `*out_b` with the required ordering. Do not return pointers to stack-local arrays.
- The generated harness provides `main` and test data. Do not add another `main`, change the signature, or rely on interactive input. Debug output is separate from structured grading results.

For example, an integer-array reduction starter has this shape:

```c
#include <stddef.h>

long long solve(const long long *nums, size_t len) {
    long long total = 0;
    for (size_t i = 0; i < len; ++i) total += nums[i];
    return total;
}
```

This illustrates the calling convention, not a solution for every array problem. Read the selected problem's contract and examples.

### Zig practice

Zig practice requires the standalone **`zig` compiler on `PATH`**. It is a separate language selection, not C compiled through `zig cc`. Implement the supplied public `solve` function without changing its signature:

- Integers use `i64`, strings use `[]const u8`, arrays use `[]const i64`, and predicates return `bool`.
- String/array-output tasks receive `std.mem.Allocator` and return an error-union slice (`![]const u8` or `![]const i64`). Allocate results using the supplied allocator; never return slices backed by a local stack array.
- Index-pair tasks return `[2]i64` in the problem's required ordering.
- The harness owns the executable entry point and test inputs. Do not add an unrelated `main` or an interactive input loop.

For example, an array reduction has this shape:

```zig
pub fn solve(nums: []const i64) i64 {
    var total: i64 = 0;
    for (nums) |value| total += value;
    return total;
}
```

Zig's standard library and compiler APIs vary between releases. If a submission fails before running test cases, inspect the compile diagnostics and detected toolchain version. Prefer the provided starter over code copied for another Zig release. Verified on this machine with **Zig 0.17.1** and **GCC 16.2.1**: real accepted and rejected submissions pass through the widget, and runner tests cover all nine exercise-signature variants, starter compilation, and unavailable-toolchain handling. Other Zig releases are not guaranteed by those results.

### Other language selections

The catalog also includes Python, JavaScript, TypeScript, Rust, C++, Go, Java, Ruby and Lua. Each language needs its own compiler/interpreter; availability is independent of the currently edited project's language. Consult the widget's missing-toolchain hint for the executable names it expects.

## Execution limits and trust

Submissions execute as **your local user** in temporary working directories. Compilation and execution have time/resource limits, bounded output capture, a sanitized environment, and process-group cancellation. Workspaces are cleaned up after completion or cancellation. A timeout or output limit is reported as a submission result rather than freezing the editor.

**This is not a filesystem or network security sandbox.** Temporary directories, environment filtering and resource limits do not make malicious source safe. Only run code you trust; do not submit unreviewed code that could read, modify or delete your files or launch external programs. No remote judging service is required, but local code may still access capabilities available to your user.

## Verification

From `/home/roubs/Project/Gtuim`:

```sh
cargo check --offline
cargo test --offline --lib gui::profile
cargo test --offline --lib gui::practice
cargo test --offline
```

The explicit `c_and_zig_verify_every_exercise_signature_accepted_rejected_templates_and_unavailable` and `widget_runs_real_c_and_zig_accepted_and_rejected_submissions` tests require working C and Zig compilers; they do not silently skip their C/Zig verification. Other multi-language tests may exercise only installed toolchains. Record detected versions and explicit execution results during peer review.

Rerun verification after shared-tree edits rather than relying on an earlier test snapshot.
