//! Curated daily coding practice catalog, languages, difficulties, typed test cases,
//! deterministic daily selection, and per-language starter templates.

use super::storage::LocalDate;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PracticeLanguage {
    Python,
    JavaScript,
    TypeScript,
    Rust,
    C,
    Cpp,
    Zig,
    Go,
    Java,
    Ruby,
    Lua,
}

impl PracticeLanguage {
    pub const ALL: &'static [PracticeLanguage] = &[
        PracticeLanguage::Python,
        PracticeLanguage::JavaScript,
        PracticeLanguage::TypeScript,
        PracticeLanguage::Rust,
        PracticeLanguage::C,
        PracticeLanguage::Cpp,
        PracticeLanguage::Zig,
        PracticeLanguage::Go,
        PracticeLanguage::Java,
        PracticeLanguage::Ruby,
        PracticeLanguage::Lua,
    ];

    pub const fn id(self) -> &'static str {
        match self {
            Self::Python => "python",
            Self::JavaScript => "javascript",
            Self::TypeScript => "typescript",
            Self::Rust => "rust",
            Self::C => "c",
            Self::Cpp => "cpp",
            Self::Zig => "zig",
            Self::Go => "go",
            Self::Java => "java",
            Self::Ruby => "ruby",
            Self::Lua => "lua",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Python => "Python",
            Self::JavaScript => "JavaScript",
            Self::TypeScript => "TypeScript",
            Self::Rust => "Rust",
            Self::C => "C",
            Self::Cpp => "C++",
            Self::Zig => "Zig",
            Self::Go => "Go",
            Self::Java => "Java",
            Self::Ruby => "Ruby",
            Self::Lua => "Lua",
        }
    }

    pub const fn short_label(self) -> &'static str {
        match self {
            Self::Python => "Py",
            Self::JavaScript => "JS",
            Self::TypeScript => "TS",
            Self::Rust => "Rust",
            Self::C => "C",
            Self::Cpp => "C++",
            Self::Zig => "Zig",
            Self::Go => "Go",
            Self::Java => "Java",
            Self::Ruby => "Ruby",
            Self::Lua => "Lua",
        }
    }

    pub const fn file_name(self) -> &'static str {
        match self {
            Self::Python => "solution.py",
            Self::JavaScript => "solution.js",
            Self::TypeScript => "solution.ts",
            Self::Rust => "solution.rs",
            Self::C => "solution.c",
            Self::Cpp => "solution.cpp",
            Self::Zig => "solution.zig",
            Self::Go => "solution.go",
            Self::Java => "Solution.java",
            Self::Ruby => "solution.rb",
            Self::Lua => "solution.lua",
        }
    }

    pub const fn indent_spaces(self) -> usize {
        match self {
            Self::JavaScript | Self::TypeScript | Self::Ruby | Self::Lua => 2,
            Self::Python
            | Self::Rust
            | Self::C
            | Self::Cpp
            | Self::Zig
            | Self::Go
            | Self::Java => 4,
        }
    }

    pub const fn install_hint(self) -> &'static str {
        match self {
            Self::Python => "Install Python 3 (`python3`) to run Python submissions.",
            Self::JavaScript => "Install Node.js (`node`) to run JavaScript submissions.",
            Self::TypeScript => {
                "Install Node.js 22.6+ (`node`) or `tsx` / `bun` / `deno` to run TypeScript."
            }
            Self::Rust => "Install Rust (`rustc`) via rustup or your system package manager.",
            Self::C => "Install a C11 compiler (`cc`, `gcc`, or `clang`) and ensure it is on PATH.",
            Self::Cpp => "Install a C++17 compiler (`g++`, `clang++`, or `c++`).",
            Self::Zig => "Install Zig (`zig`) via your package manager or mise and ensure it is on PATH.",
            Self::Go => "Install the Go toolchain (`go`) to run Go submissions.",
            Self::Java => "Install a JDK (`javac` and `java`) to run Java submissions.",
            Self::Ruby => "Install Ruby (`ruby`) to run Ruby submissions.",
            Self::Lua => "Install Lua (`lua` or `luajit`) to run Lua submissions.",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|l| l.id().eq_ignore_ascii_case(id))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Difficulty {
    Easy,
    Medium,
    Hard,
}

impl Difficulty {
    pub const ALL: &'static [Difficulty] =
        &[Difficulty::Easy, Difficulty::Medium, Difficulty::Hard];

    pub const fn id(self) -> &'static str {
        match self {
            Self::Easy => "easy",
            Self::Medium => "medium",
            Self::Hard => "hard",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Easy => "Easy",
            Self::Medium => "Medium",
            Self::Hard => "Hard",
        }
    }

    pub const fn estimated_minutes(self) -> &'static str {
        match self {
            Self::Easy => "~5 min",
            Self::Medium => "~8 min",
            Self::Hard => "~12 min",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|d| d.id().eq_ignore_ascii_case(id))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExerciseSignature {
    IntToInt,
    StrToBool,
    StrToInt,
    StrToStr,
    TwoStrsToBool,
    IntVecToInt,
    IntVecIntToInt,
    IntVecToIntVec,
    IntVecIntToPair,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TestInput {
    Int(i64),
    Str(String),
    TwoStrs(String, String),
    IntVec(Vec<i64>),
    IntVecAndInt(Vec<i64>, i64),
}

impl TestInput {
    pub fn display(&self) -> String {
        match self {
            Self::Int(n) => n.to_string(),
            Self::Str(s) => format!("{s:?}"),
            Self::TwoStrs(a, b) => format!("a = {a:?}, b = {b:?}"),
            Self::IntVec(v) => format!("{v:?}"),
            Self::IntVecAndInt(v, k) => format!("nums = {v:?}, k = {k}"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TestExpected {
    Int(i64),
    Bool(bool),
    Str(String),
    IntVec(Vec<i64>),
    Pair(i64, i64),
}

impl TestExpected {
    pub fn display(&self) -> String {
        match self {
            Self::Int(n) => n.to_string(),
            Self::Bool(b) => b.to_string(),
            Self::Str(s) => format!("{s:?}"),
            Self::IntVec(v) => format!("{v:?}"),
            Self::Pair(a, b) => format!("[{a}, {b}]"),
        }
    }

    /// Canonical serialized form shared across all language harnesses for exact equality.
    pub fn canonical(&self) -> String {
        match self {
            Self::Int(n) => n.to_string(),
            Self::Bool(b) => if *b { "true" } else { "false" }.to_string(),
            Self::Str(s) => serde_json::to_string(s).unwrap_or_else(|_| format!("{s:?}")),
            Self::IntVec(v) => {
                let inner: Vec<String> = v.iter().map(|x| x.to_string()).collect();
                format!("[{}]", inner.join(","))
            }
            Self::Pair(a, b) => format!("[{a},{b}]"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TestCase {
    pub input: TestInput,
    pub expected: TestExpected,
    pub hidden: bool,
}

#[derive(Clone, Debug)]
pub struct Exercise {
    pub id: &'static str,
    pub title: &'static str,
    pub difficulty: Difficulty,
    pub signature: ExerciseSignature,
    pub description: &'static str,
    pub constraints: &'static str,
    pub tests: Vec<TestCase>,
}

impl Exercise {
    pub fn visible_examples(&self) -> Vec<(String, String)> {
        self.tests
            .iter()
            .filter(|t| !t.hidden)
            .map(|t| (t.input.display(), t.expected.display()))
            .collect()
    }
}

/// Deterministic daily index for `(local_date, difficulty)`.
/// All languages share the exact same daily exercise on a given date and difficulty,
/// while preserving independent language-specific starter templates and drafts.
pub fn daily_exercise_index(
    date: LocalDate,
    difficulty: Difficulty,
    pool_len: usize,
) -> usize {
    if pool_len == 0 {
        return 0;
    }
    let key = format!("{}:{}", date, difficulty.id());
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in key.bytes() {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    (hash as usize) % pool_len
}

pub fn exercises_for_difficulty(difficulty: Difficulty) -> Vec<Exercise> {
    all_exercises()
        .into_iter()
        .filter(|e| e.difficulty == difficulty)
        .collect()
}

pub fn daily_exercise(date: LocalDate, difficulty: Difficulty) -> Exercise {
    let pool = exercises_for_difficulty(difficulty);
    let idx = daily_exercise_index(date, difficulty, pool.len());
    pool.into_iter().nth(idx).expect("non-empty difficulty pool")
}

pub fn starter_code(exercise: &Exercise, lang: PracticeLanguage) -> String {
    let title = exercise.title;
    match (lang, exercise.signature) {
        // ── Python ──────────────────────────────────────────────────────────
        (PracticeLanguage::Python, ExerciseSignature::IntToInt) => format!(
            "# {title}\ndef solve(n: int) -> int:\n    # Write your solution here\n    return 0\n"
        ),
        (PracticeLanguage::Python, ExerciseSignature::StrToBool) => format!(
            "# {title}\ndef solve(s: str) -> bool:\n    # Write your solution here\n    return False\n"
        ),
        (PracticeLanguage::Python, ExerciseSignature::StrToInt) => format!(
            "# {title}\ndef solve(s: str) -> int:\n    # Write your solution here\n    return 0\n"
        ),
        (PracticeLanguage::Python, ExerciseSignature::StrToStr) => format!(
            "# {title}\ndef solve(s: str) -> str:\n    # Write your solution here\n    return \"\"\n"
        ),
        (PracticeLanguage::Python, ExerciseSignature::TwoStrsToBool) => format!(
            "# {title}\ndef solve(a: str, b: str) -> bool:\n    # Write your solution here\n    return False\n"
        ),
        (PracticeLanguage::Python, ExerciseSignature::IntVecToInt) => format!(
            "# {title}\ndef solve(nums: list[int]) -> int:\n    # Write your solution here\n    return 0\n"
        ),
        (PracticeLanguage::Python, ExerciseSignature::IntVecIntToInt) => format!(
            "# {title}\ndef solve(nums: list[int], k: int) -> int:\n    # Write your solution here\n    return 0\n"
        ),
        (PracticeLanguage::Python, ExerciseSignature::IntVecToIntVec) => format!(
            "# {title}\ndef solve(nums: list[int]) -> list[int]:\n    # Write your solution here\n    return []\n"
        ),
        (PracticeLanguage::Python, ExerciseSignature::IntVecIntToPair) => format!(
            "# {title}\ndef solve(nums: list[int], k: int) -> list[int]:\n    # Return [i, j] with i < j\n    return [0, 0]\n"
        ),

        // ── JavaScript ──────────────────────────────────────────────────────
        (PracticeLanguage::JavaScript, ExerciseSignature::IntToInt) => format!(
            "// {title}\nfunction solve(n) {{\n  return 0;\n}}\n"
        ),
        (PracticeLanguage::JavaScript, ExerciseSignature::StrToBool) => format!(
            "// {title}\nfunction solve(s) {{\n  return false;\n}}\n"
        ),
        (PracticeLanguage::JavaScript, ExerciseSignature::StrToInt) => format!(
            "// {title}\nfunction solve(s) {{\n  return 0;\n}}\n"
        ),
        (PracticeLanguage::JavaScript, ExerciseSignature::StrToStr) => format!(
            "// {title}\nfunction solve(s) {{\n  return \"\";\n}}\n"
        ),
        (PracticeLanguage::JavaScript, ExerciseSignature::TwoStrsToBool) => format!(
            "// {title}\nfunction solve(a, b) {{\n  return false;\n}}\n"
        ),
        (PracticeLanguage::JavaScript, ExerciseSignature::IntVecToInt) => format!(
            "// {title}\nfunction solve(nums) {{\n  return 0;\n}}\n"
        ),
        (PracticeLanguage::JavaScript, ExerciseSignature::IntVecIntToInt) => format!(
            "// {title}\nfunction solve(nums, k) {{\n  return 0;\n}}\n"
        ),
        (PracticeLanguage::JavaScript, ExerciseSignature::IntVecToIntVec) => format!(
            "// {title}\nfunction solve(nums) {{\n  return [];\n}}\n"
        ),
        (PracticeLanguage::JavaScript, ExerciseSignature::IntVecIntToPair) => format!(
            "// {title}\nfunction solve(nums, k) {{\n  return [0, 0];\n}}\n"
        ),

        // ── TypeScript ──────────────────────────────────────────────────────
        (PracticeLanguage::TypeScript, ExerciseSignature::IntToInt) => format!(
            "// {title}\nfunction solve(n: number): number {{\n  return 0;\n}}\n"
        ),
        (PracticeLanguage::TypeScript, ExerciseSignature::StrToBool) => format!(
            "// {title}\nfunction solve(s: string): boolean {{\n  return false;\n}}\n"
        ),
        (PracticeLanguage::TypeScript, ExerciseSignature::StrToInt) => format!(
            "// {title}\nfunction solve(s: string): number {{\n  return 0;\n}}\n"
        ),
        (PracticeLanguage::TypeScript, ExerciseSignature::StrToStr) => format!(
            "// {title}\nfunction solve(s: string): string {{\n  return \"\";\n}}\n"
        ),
        (PracticeLanguage::TypeScript, ExerciseSignature::TwoStrsToBool) => format!(
            "// {title}\nfunction solve(a: string, b: string): boolean {{\n  return false;\n}}\n"
        ),
        (PracticeLanguage::TypeScript, ExerciseSignature::IntVecToInt) => format!(
            "// {title}\nfunction solve(nums: number[]): number {{\n  return 0;\n}}\n"
        ),
        (PracticeLanguage::TypeScript, ExerciseSignature::IntVecIntToInt) => format!(
            "// {title}\nfunction solve(nums: number[], k: number): number {{\n  return 0;\n}}\n"
        ),
        (PracticeLanguage::TypeScript, ExerciseSignature::IntVecToIntVec) => format!(
            "// {title}\nfunction solve(nums: number[]): number[] {{\n  return [];\n}}\n"
        ),
        (PracticeLanguage::TypeScript, ExerciseSignature::IntVecIntToPair) => format!(
            "// {title}\nfunction solve(nums: number[], k: number): [number, number] {{\n  return [0, 0];\n}}\n"
        ),

        // ── Rust ────────────────────────────────────────────────────────────
        (PracticeLanguage::Rust, ExerciseSignature::IntToInt) => format!(
            "// {title}\npub fn solve(n: i64) -> i64 {{\n    0\n}}\n"
        ),
        (PracticeLanguage::Rust, ExerciseSignature::StrToBool) => format!(
            "// {title}\npub fn solve(s: &str) -> bool {{\n    false\n}}\n"
        ),
        (PracticeLanguage::Rust, ExerciseSignature::StrToInt) => format!(
            "// {title}\npub fn solve(s: &str) -> i64 {{\n    0\n}}\n"
        ),
        (PracticeLanguage::Rust, ExerciseSignature::StrToStr) => format!(
            "// {title}\npub fn solve(s: &str) -> String {{\n    String::new()\n}}\n"
        ),
        (PracticeLanguage::Rust, ExerciseSignature::TwoStrsToBool) => format!(
            "// {title}\npub fn solve(a: &str, b: &str) -> bool {{\n    false\n}}\n"
        ),
        (PracticeLanguage::Rust, ExerciseSignature::IntVecToInt) => format!(
            "// {title}\npub fn solve(nums: &[i64]) -> i64 {{\n    0\n}}\n"
        ),
        (PracticeLanguage::Rust, ExerciseSignature::IntVecIntToInt) => format!(
            "// {title}\npub fn solve(nums: &[i64], k: i64) -> i64 {{\n    0\n}}\n"
        ),
        (PracticeLanguage::Rust, ExerciseSignature::IntVecToIntVec) => format!(
            "// {title}\npub fn solve(nums: &[i64]) -> Vec<i64> {{\n    Vec::new()\n}}\n"
        ),
        (PracticeLanguage::Rust, ExerciseSignature::IntVecIntToPair) => format!(
            "// {title}\npub fn solve(nums: &[i64], k: i64) -> (i64, i64) {{\n    (0, 0)\n}}\n"
        ),

        // ── C ───────────────────────────────────────────────────────────────
        (PracticeLanguage::C, ExerciseSignature::IntToInt) => format!(
            "// {title}\n#include <stdbool.h>\n#include <stddef.h>\n\nlong long solve(long long n) {{\n    (void)n;\n    return 0;\n}}\n"
        ),
        (PracticeLanguage::C, ExerciseSignature::StrToBool) => format!(
            "// {title}\n#include <stdbool.h>\n#include <stddef.h>\n#include <string.h>\n\nbool solve(const char *s) {{\n    (void)s;\n    return false;\n}}\n"
        ),
        (PracticeLanguage::C, ExerciseSignature::StrToInt) => format!(
            "// {title}\n#include <stddef.h>\n#include <string.h>\n\nlong long solve(const char *s) {{\n    (void)s;\n    return 0;\n}}\n"
        ),
        (PracticeLanguage::C, ExerciseSignature::StrToStr) => format!(
            "// {title}\n#include <stddef.h>\n#include <string.h>\n\n// Write null-terminated output into `out` (capacity `out_cap`).\nvoid solve(const char *s, char *out, size_t out_cap) {{\n    (void)s;\n    if (out_cap > 0) out[0] = '\\0';\n}}\n"
        ),
        (PracticeLanguage::C, ExerciseSignature::TwoStrsToBool) => format!(
            "// {title}\n#include <stdbool.h>\n#include <stddef.h>\n#include <string.h>\n\nbool solve(const char *a, const char *b) {{\n    (void)a; (void)b;\n    return false;\n}}\n"
        ),
        (PracticeLanguage::C, ExerciseSignature::IntVecToInt) => format!(
            "// {title}\n#include <stddef.h>\n\nlong long solve(const long long *nums, size_t len) {{\n    (void)nums; (void)len;\n    return 0;\n}}\n"
        ),
        (PracticeLanguage::C, ExerciseSignature::IntVecIntToInt) => format!(
            "// {title}\n#include <stddef.h>\n\nlong long solve(const long long *nums, size_t len, long long k) {{\n    (void)nums; (void)len; (void)k;\n    return 0;\n}}\n"
        ),
        (PracticeLanguage::C, ExerciseSignature::IntVecToIntVec) => format!(
            "// {title}\n#include <stddef.h>\n\n// Write result values into `out` and return the result length.\nsize_t solve(const long long *nums, size_t len, long long *out) {{\n    (void)nums; (void)len; (void)out;\n    return 0;\n}}\n"
        ),
        (PracticeLanguage::C, ExerciseSignature::IntVecIntToPair) => format!(
            "// {title}\n#include <stddef.h>\n\n// Store the index pair (i < j) in *out_a and *out_b.\nvoid solve(const long long *nums, size_t len, long long k, long long *out_a, long long *out_b) {{\n    (void)nums; (void)len; (void)k;\n    *out_a = 0;\n    *out_b = 0;\n}}\n"
        ),

        // ── C++ ─────────────────────────────────────────────────────────────
        (PracticeLanguage::Cpp, ExerciseSignature::IntToInt) => format!(
            "// {title}\n#include <bits/stdc++.h>\nusing namespace std;\n\nlong long solve(long long n) {{\n    return 0;\n}}\n"
        ),
        (PracticeLanguage::Cpp, ExerciseSignature::StrToBool) => format!(
            "// {title}\n#include <bits/stdc++.h>\nusing namespace std;\n\nbool solve(const string& s) {{\n    return false;\n}}\n"
        ),
        (PracticeLanguage::Cpp, ExerciseSignature::StrToInt) => format!(
            "// {title}\n#include <bits/stdc++.h>\nusing namespace std;\n\nlong long solve(const string& s) {{\n    return 0;\n}}\n"
        ),
        (PracticeLanguage::Cpp, ExerciseSignature::StrToStr) => format!(
            "// {title}\n#include <bits/stdc++.h>\nusing namespace std;\n\nstring solve(const string& s) {{\n    return \"\";\n}}\n"
        ),
        (PracticeLanguage::Cpp, ExerciseSignature::TwoStrsToBool) => format!(
            "// {title}\n#include <bits/stdc++.h>\nusing namespace std;\n\nbool solve(const string& a, const string& b) {{\n    return false;\n}}\n"
        ),
        (PracticeLanguage::Cpp, ExerciseSignature::IntVecToInt) => format!(
            "// {title}\n#include <bits/stdc++.h>\nusing namespace std;\n\nlong long solve(const vector<long long>& nums) {{\n    return 0;\n}}\n"
        ),
        (PracticeLanguage::Cpp, ExerciseSignature::IntVecIntToInt) => format!(
            "// {title}\n#include <bits/stdc++.h>\nusing namespace std;\n\nlong long solve(const vector<long long>& nums, long long k) {{\n    return 0;\n}}\n"
        ),
        (PracticeLanguage::Cpp, ExerciseSignature::IntVecToIntVec) => format!(
            "// {title}\n#include <bits/stdc++.h>\nusing namespace std;\n\nvector<long long> solve(const vector<long long>& nums) {{\n    return {{}};\n}}\n"
        ),
        (PracticeLanguage::Cpp, ExerciseSignature::IntVecIntToPair) => format!(
            "// {title}\n#include <bits/stdc++.h>\nusing namespace std;\n\npair<long long, long long> solve(const vector<long long>& nums, long long k) {{\n    return {{0, 0}};\n}}\n"
        ),

        // ── Zig ─────────────────────────────────────────────────────────────
        (PracticeLanguage::Zig, ExerciseSignature::IntToInt) => format!(
            "// {title}\npub fn solve(n: i64) i64 {{\n    _ = n;\n    return 0;\n}}\n"
        ),
        (PracticeLanguage::Zig, ExerciseSignature::StrToBool) => format!(
            "// {title}\npub fn solve(s: []const u8) bool {{\n    _ = s;\n    return false;\n}}\n"
        ),
        (PracticeLanguage::Zig, ExerciseSignature::StrToInt) => format!(
            "// {title}\npub fn solve(s: []const u8) i64 {{\n    _ = s;\n    return 0;\n}}\n"
        ),
        (PracticeLanguage::Zig, ExerciseSignature::StrToStr) => format!(
            "// {title}\nconst std = @import(\"std\");\n\npub fn solve(alloc: std.mem.Allocator, s: []const u8) ![]const u8 {{\n    _ = alloc;\n    _ = s;\n    return \"\";\n}}\n"
        ),
        (PracticeLanguage::Zig, ExerciseSignature::TwoStrsToBool) => format!(
            "// {title}\npub fn solve(a: []const u8, b: []const u8) bool {{\n    _ = a;\n    _ = b;\n    return false;\n}}\n"
        ),
        (PracticeLanguage::Zig, ExerciseSignature::IntVecToInt) => format!(
            "// {title}\npub fn solve(nums: []const i64) i64 {{\n    _ = nums;\n    return 0;\n}}\n"
        ),
        (PracticeLanguage::Zig, ExerciseSignature::IntVecIntToInt) => format!(
            "// {title}\npub fn solve(nums: []const i64, k: i64) i64 {{\n    _ = nums;\n    _ = k;\n    return 0;\n}}\n"
        ),
        (PracticeLanguage::Zig, ExerciseSignature::IntVecToIntVec) => format!(
            "// {title}\nconst std = @import(\"std\");\n\npub fn solve(alloc: std.mem.Allocator, nums: []const i64) ![]const i64 {{\n    _ = alloc;\n    _ = nums;\n    return &.{{}};\n}}\n"
        ),
        (PracticeLanguage::Zig, ExerciseSignature::IntVecIntToPair) => format!(
            "// {title}\npub fn solve(nums: []const i64, k: i64) [2]i64 {{\n    _ = nums;\n    _ = k;\n    return .{{ 0, 0 }};\n}}\n"
        ),

        // ── Go ──────────────────────────────────────────────────────────────
        (PracticeLanguage::Go, ExerciseSignature::IntToInt) => format!(
            "// {title}\npackage main\n\nfunc solve(n int64) int64 {{\n    return 0\n}}\n"
        ),
        (PracticeLanguage::Go, ExerciseSignature::StrToBool) => format!(
            "// {title}\npackage main\n\nfunc solve(s string) bool {{\n    return false\n}}\n"
        ),
        (PracticeLanguage::Go, ExerciseSignature::StrToInt) => format!(
            "// {title}\npackage main\n\nfunc solve(s string) int64 {{\n    return 0\n}}\n"
        ),
        (PracticeLanguage::Go, ExerciseSignature::StrToStr) => format!(
            "// {title}\npackage main\n\nfunc solve(s string) string {{\n    return \"\"\n}}\n"
        ),
        (PracticeLanguage::Go, ExerciseSignature::TwoStrsToBool) => format!(
            "// {title}\npackage main\n\nfunc solve(a, b string) bool {{\n    return false\n}}\n"
        ),
        (PracticeLanguage::Go, ExerciseSignature::IntVecToInt) => format!(
            "// {title}\npackage main\n\nfunc solve(nums []int64) int64 {{\n    return 0\n}}\n"
        ),
        (PracticeLanguage::Go, ExerciseSignature::IntVecIntToInt) => format!(
            "// {title}\npackage main\n\nfunc solve(nums []int64, k int64) int64 {{\n    return 0\n}}\n"
        ),
        (PracticeLanguage::Go, ExerciseSignature::IntVecToIntVec) => format!(
            "// {title}\npackage main\n\nfunc solve(nums []int64) []int64 {{\n    return []int64{{}}\n}}\n"
        ),
        (PracticeLanguage::Go, ExerciseSignature::IntVecIntToPair) => format!(
            "// {title}\npackage main\n\nfunc solve(nums []int64, k int64) [2]int64 {{\n    return [2]int64{{0, 0}}\n}}\n"
        ),

        // ── Java ────────────────────────────────────────────────────────────
        (PracticeLanguage::Java, ExerciseSignature::IntToInt) => format!(
            "// {title}\npublic class Solution {{\n    public static long solve(long n) {{\n        return 0;\n    }}\n}}\n"
        ),
        (PracticeLanguage::Java, ExerciseSignature::StrToBool) => format!(
            "// {title}\npublic class Solution {{\n    public static boolean solve(String s) {{\n        return false;\n    }}\n}}\n"
        ),
        (PracticeLanguage::Java, ExerciseSignature::StrToInt) => format!(
            "// {title}\npublic class Solution {{\n    public static long solve(String s) {{\n        return 0;\n    }}\n}}\n"
        ),
        (PracticeLanguage::Java, ExerciseSignature::StrToStr) => format!(
            "// {title}\npublic class Solution {{\n    public static String solve(String s) {{\n        return \"\";\n    }}\n}}\n"
        ),
        (PracticeLanguage::Java, ExerciseSignature::TwoStrsToBool) => format!(
            "// {title}\npublic class Solution {{\n    public static boolean solve(String a, String b) {{\n        return false;\n    }}\n}}\n"
        ),
        (PracticeLanguage::Java, ExerciseSignature::IntVecToInt) => format!(
            "// {title}\npublic class Solution {{\n    public static long solve(long[] nums) {{\n        return 0;\n    }}\n}}\n"
        ),
        (PracticeLanguage::Java, ExerciseSignature::IntVecIntToInt) => format!(
            "// {title}\npublic class Solution {{\n    public static long solve(long[] nums, long k) {{\n        return 0;\n    }}\n}}\n"
        ),
        (PracticeLanguage::Java, ExerciseSignature::IntVecToIntVec) => format!(
            "// {title}\npublic class Solution {{\n    public static long[] solve(long[] nums) {{\n        return new long[0];\n    }}\n}}\n"
        ),
        (PracticeLanguage::Java, ExerciseSignature::IntVecIntToPair) => format!(
            "// {title}\npublic class Solution {{\n    public static long[] solve(long[] nums, long k) {{\n        return new long[]{{0, 0}};\n    }}\n}}\n"
        ),

        // ── Ruby ────────────────────────────────────────────────────────────
        (PracticeLanguage::Ruby, ExerciseSignature::IntToInt) => {
            format!("# {title}\ndef solve(n)\n  0\nend\n")
        }
        (PracticeLanguage::Ruby, ExerciseSignature::StrToBool) => {
            format!("# {title}\ndef solve(s)\n  false\nend\n")
        }
        (PracticeLanguage::Ruby, ExerciseSignature::StrToInt) => {
            format!("# {title}\ndef solve(s)\n  0\nend\n")
        }
        (PracticeLanguage::Ruby, ExerciseSignature::StrToStr) => {
            format!("# {title}\ndef solve(s)\n  \"\"\nend\n")
        }
        (PracticeLanguage::Ruby, ExerciseSignature::TwoStrsToBool) => {
            format!("# {title}\ndef solve(a, b)\n  false\nend\n")
        }
        (PracticeLanguage::Ruby, ExerciseSignature::IntVecToInt) => {
            format!("# {title}\ndef solve(nums)\n  0\nend\n")
        }
        (PracticeLanguage::Ruby, ExerciseSignature::IntVecIntToInt) => {
            format!("# {title}\ndef solve(nums, k)\n  0\nend\n")
        }
        (PracticeLanguage::Ruby, ExerciseSignature::IntVecToIntVec) => {
            format!("# {title}\ndef solve(nums)\n  []\nend\n")
        }
        (PracticeLanguage::Ruby, ExerciseSignature::IntVecIntToPair) => {
            format!("# {title}\ndef solve(nums, k)\n  [0, 0]\nend\n")
        }

        // ── Lua ─────────────────────────────────────────────────────────────
        (PracticeLanguage::Lua, ExerciseSignature::IntToInt) => {
            format!("-- {title}\nfunction solve(n)\n  return 0\nend\n")
        }
        (PracticeLanguage::Lua, ExerciseSignature::StrToBool) => {
            format!("-- {title}\nfunction solve(s)\n  return false\nend\n")
        }
        (PracticeLanguage::Lua, ExerciseSignature::StrToInt) => {
            format!("-- {title}\nfunction solve(s)\n  return 0\nend\n")
        }
        (PracticeLanguage::Lua, ExerciseSignature::StrToStr) => {
            format!("-- {title}\nfunction solve(s)\n  return \"\"\nend\n")
        }
        (PracticeLanguage::Lua, ExerciseSignature::TwoStrsToBool) => {
            format!("-- {title}\nfunction solve(a, b)\n  return false\nend\n")
        }
        (PracticeLanguage::Lua, ExerciseSignature::IntVecToInt) => {
            format!("-- {title}\nfunction solve(nums)\n  return 0\nend\n")
        }
        (PracticeLanguage::Lua, ExerciseSignature::IntVecIntToInt) => {
            format!("-- {title}\nfunction solve(nums, k)\n  return 0\nend\n")
        }
        (PracticeLanguage::Lua, ExerciseSignature::IntVecToIntVec) => {
            format!("-- {title}\nfunction solve(nums)\n  return {{}}\nend\n")
        }
        (PracticeLanguage::Lua, ExerciseSignature::IntVecIntToPair) => {
            format!("-- {title} (return 0-based indices {{i, j}})\nfunction solve(nums, k)\n  return {{0, 0}}\nend\n")
        }
    }
}

pub fn all_exercises() -> Vec<Exercise> {
    vec![
        // ════════════════════════════════════════════════════════════════════
        // EASY (6)
        // ════════════════════════════════════════════════════════════════════
        Exercise {
            id: "valid-parentheses",
            title: "Valid Parentheses",
            difficulty: Difficulty::Easy,
            signature: ExerciseSignature::StrToBool,
            description: "Given a string containing only '(', ')', '{', '}', '[' and ']', determine whether every opening bracket is closed by the same type in the correct nesting order.",
            constraints: "0 <= len(s) <= 10^4; characters are only ()[]{}",
            tests: vec![
                TestCase { input: TestInput::Str("()[]{}".into()), expected: TestExpected::Bool(true), hidden: false },
                TestCase { input: TestInput::Str("([)]".into()), expected: TestExpected::Bool(false), hidden: false },
                TestCase { input: TestInput::Str("{[()]}".into()), expected: TestExpected::Bool(true), hidden: false },
                TestCase { input: TestInput::Str("".into()), expected: TestExpected::Bool(true), hidden: true },
                TestCase { input: TestInput::Str("(".into()), expected: TestExpected::Bool(false), hidden: true },
                TestCase { input: TestInput::Str("]".into()), expected: TestExpected::Bool(false), hidden: true },
                TestCase { input: TestInput::Str("((({{{[[[]]]}}})))".into()), expected: TestExpected::Bool(true), hidden: true },
                TestCase { input: TestInput::Str("((({{{[[[]]]}}}))".into()), expected: TestExpected::Bool(false), hidden: true },
            ],
        },
        Exercise {
            id: "two-sum",
            title: "Two Sum Indices",
            difficulty: Difficulty::Easy,
            signature: ExerciseSignature::IntVecIntToPair,
            description: "Given an array of integers `nums` and an integer target `k`, return the unique 0-based index pair [i, j] with i < j such that nums[i] + nums[j] == k.",
            constraints: "2 <= len(nums) <= 10^4; exactly one valid pair exists.",
            tests: vec![
                TestCase { input: TestInput::IntVecAndInt(vec![2, 7, 11, 15], 9), expected: TestExpected::Pair(0, 1), hidden: false },
                TestCase { input: TestInput::IntVecAndInt(vec![3, 2, 4], 6), expected: TestExpected::Pair(1, 2), hidden: false },
                TestCase { input: TestInput::IntVecAndInt(vec![3, 3], 6), expected: TestExpected::Pair(0, 1), hidden: false },
                TestCase { input: TestInput::IntVecAndInt(vec![-5, 1, 9, 4, -2], 2), expected: TestExpected::Pair(3, 4), hidden: true },
                TestCase { input: TestInput::IntVecAndInt(vec![0, 4, 3, 0], 0), expected: TestExpected::Pair(0, 3), hidden: true },
                TestCase { input: TestInput::IntVecAndInt(vec![10, 22, 5, 75, 65, 80], 140), expected: TestExpected::Pair(3, 4), hidden: true },
                TestCase { input: TestInput::IntVecAndInt(vec![-10, -3, -1, 8, 12], -13), expected: TestExpected::Pair(0, 1), hidden: true },
            ],
        },
        Exercise {
            id: "valid-anagram",
            title: "Valid Anagram",
            difficulty: Difficulty::Easy,
            signature: ExerciseSignature::TwoStrsToBool,
            description: "Return true if string `b` is an exact anagram of string `a` (same character counts, case-sensitive), otherwise return false.",
            constraints: "0 <= len(a), len(b) <= 10^4; printable ASCII characters.",
            tests: vec![
                TestCase { input: TestInput::TwoStrs("anagram".into(), "nagaram".into()), expected: TestExpected::Bool(true), hidden: false },
                TestCase { input: TestInput::TwoStrs("rat".into(), "car".into()), expected: TestExpected::Bool(false), hidden: false },
                TestCase { input: TestInput::TwoStrs("listen".into(), "silent".into()), expected: TestExpected::Bool(true), hidden: false },
                TestCase { input: TestInput::TwoStrs("".into(), "".into()), expected: TestExpected::Bool(true), hidden: true },
                TestCase { input: TestInput::TwoStrs("a".into(), "ab".into()), expected: TestExpected::Bool(false), hidden: true },
                TestCase { input: TestInput::TwoStrs("aabbcc".into(), "abcabc".into()), expected: TestExpected::Bool(true), hidden: true },
                TestCase { input: TestInput::TwoStrs("Aa".into(), "aa".into()), expected: TestExpected::Bool(false), hidden: true },
            ],
        },
        Exercise {
            id: "move-zeroes",
            title: "Move Zeroes",
            difficulty: Difficulty::Easy,
            signature: ExerciseSignature::IntVecToIntVec,
            description: "Return a new array with all `0` elements moved to the end while preserving the relative order of all non-zero elements.",
            constraints: "0 <= len(nums) <= 10^4.",
            tests: vec![
                TestCase { input: TestInput::IntVec(vec![0, 1, 0, 3, 12]), expected: TestExpected::IntVec(vec![1, 3, 12, 0, 0]), hidden: false },
                TestCase { input: TestInput::IntVec(vec![0]), expected: TestExpected::IntVec(vec![0]), hidden: false },
                TestCase { input: TestInput::IntVec(vec![4, 2, 0, 0, -1, 0, 5]), expected: TestExpected::IntVec(vec![4, 2, -1, 5, 0, 0, 0]), hidden: false },
                TestCase { input: TestInput::IntVec(vec![]), expected: TestExpected::IntVec(vec![]), hidden: true },
                TestCase { input: TestInput::IntVec(vec![0, 0, 0]), expected: TestExpected::IntVec(vec![0, 0, 0]), hidden: true },
                TestCase { input: TestInput::IntVec(vec![7, 8, 9]), expected: TestExpected::IntVec(vec![7, 8, 9]), hidden: true },
                TestCase { input: TestInput::IntVec(vec![0, 0, 1]), expected: TestExpected::IntVec(vec![1, 0, 0]), hidden: true },
            ],
        },
        Exercise {
            id: "best-stock-profit",
            title: "Best Stock Profit",
            difficulty: Difficulty::Easy,
            signature: ExerciseSignature::IntVecToInt,
            description: "Given daily stock prices `nums`, compute the maximum profit from buying on one day and selling on a later day. Return 0 if no profitable transaction is possible.",
            constraints: "0 <= len(nums) <= 10^5; 0 <= nums[i] <= 10^9.",
            tests: vec![
                TestCase { input: TestInput::IntVec(vec![7, 1, 5, 3, 6, 4]), expected: TestExpected::Int(5), hidden: false },
                TestCase { input: TestInput::IntVec(vec![7, 6, 4, 3, 1]), expected: TestExpected::Int(0), hidden: false },
                TestCase { input: TestInput::IntVec(vec![2, 4, 1, 7]), expected: TestExpected::Int(6), hidden: false },
                TestCase { input: TestInput::IntVec(vec![]), expected: TestExpected::Int(0), hidden: true },
                TestCase { input: TestInput::IntVec(vec![42]), expected: TestExpected::Int(0), hidden: true },
                TestCase { input: TestInput::IntVec(vec![3, 3, 3]), expected: TestExpected::Int(0), hidden: true },
                TestCase { input: TestInput::IntVec(vec![10, 2, 11, 1, 9]), expected: TestExpected::Int(9), hidden: true },
            ],
        },
        Exercise {
            id: "digital-root",
            title: "Digital Root",
            difficulty: Difficulty::Easy,
            signature: ExerciseSignature::IntToInt,
            description: "Repeatedly sum the base-10 digits of a non-negative integer `n` until a single digit (0..=9) remains, and return that digit.",
            constraints: "0 <= n <= 10^15.",
            tests: vec![
                TestCase { input: TestInput::Int(38), expected: TestExpected::Int(2), hidden: false },
                TestCase { input: TestInput::Int(0), expected: TestExpected::Int(0), hidden: false },
                TestCase { input: TestInput::Int(999), expected: TestExpected::Int(9), hidden: false },
                TestCase { input: TestInput::Int(7), expected: TestExpected::Int(7), hidden: true },
                TestCase { input: TestInput::Int(10), expected: TestExpected::Int(1), hidden: true },
                TestCase { input: TestInput::Int(123456789), expected: TestExpected::Int(9), hidden: true },
                TestCase { input: TestInput::Int(9876543210123), expected: TestExpected::Int(6), hidden: true },
            ],
        },

        // ════════════════════════════════════════════════════════════════════
        // MEDIUM (6)
        // ════════════════════════════════════════════════════════════════════
        Exercise {
            id: "longest-unique-substr",
            title: "Longest Unique Substring",
            difficulty: Difficulty::Medium,
            signature: ExerciseSignature::StrToInt,
            description: "Given a string `s`, return the length of the longest contiguous substring without any repeated characters.",
            constraints: "0 <= len(s) <= 10^5; ASCII characters.",
            tests: vec![
                TestCase { input: TestInput::Str("abcabcbb".into()), expected: TestExpected::Int(3), hidden: false },
                TestCase { input: TestInput::Str("bbbbb".into()), expected: TestExpected::Int(1), hidden: false },
                TestCase { input: TestInput::Str("pwwkew".into()), expected: TestExpected::Int(3), hidden: false },
                TestCase { input: TestInput::Str("".into()), expected: TestExpected::Int(0), hidden: true },
                TestCase { input: TestInput::Str("dvdf".into()), expected: TestExpected::Int(3), hidden: true },
                TestCase { input: TestInput::Str("abba".into()), expected: TestExpected::Int(2), hidden: true },
                TestCase { input: TestInput::Str("abcdefg".into()), expected: TestExpected::Int(7), hidden: true },
                TestCase { input: TestInput::Str("tmmzuxt".into()), expected: TestExpected::Int(5), hidden: true },
            ],
        },
        Exercise {
            id: "max-subarray-sum",
            title: "Maximum Subarray Sum",
            difficulty: Difficulty::Medium,
            signature: ExerciseSignature::IntVecToInt,
            description: "Find the non-empty contiguous subarray of `nums` with the largest sum and return that sum.",
            constraints: "1 <= len(nums) <= 10^5; values may be negative.",
            tests: vec![
                TestCase { input: TestInput::IntVec(vec![-2, 1, -3, 4, -1, 2, 1, -5, 4]), expected: TestExpected::Int(6), hidden: false },
                TestCase { input: TestInput::IntVec(vec![1]), expected: TestExpected::Int(1), hidden: false },
                TestCase { input: TestInput::IntVec(vec![5, 4, -1, 7, 8]), expected: TestExpected::Int(23), hidden: false },
                TestCase { input: TestInput::IntVec(vec![-5, -2, -9, -1, -4]), expected: TestExpected::Int(-1), hidden: true },
                TestCase { input: TestInput::IntVec(vec![-100]), expected: TestExpected::Int(-100), hidden: true },
                TestCase { input: TestInput::IntVec(vec![0, -1, 0, -2, 0]), expected: TestExpected::Int(0), hidden: true },
                TestCase { input: TestInput::IntVec(vec![8, -19, 5, -4, 20]), expected: TestExpected::Int(21), hidden: true },
            ],
        },
        Exercise {
            id: "product-except-self",
            title: "Product Except Self",
            difficulty: Difficulty::Medium,
            signature: ExerciseSignature::IntVecToIntVec,
            description: "Return an array `out` where `out[i]` equals the product of all elements of `nums` except `nums[i]`. Handle zeros cleanly without dividing by zero.",
            constraints: "2 <= len(nums) <= 10^4; products fit in 64-bit signed integers.",
            tests: vec![
                TestCase { input: TestInput::IntVec(vec![1, 2, 3, 4]), expected: TestExpected::IntVec(vec![24, 12, 8, 6]), hidden: false },
                TestCase { input: TestInput::IntVec(vec![-1, 1, 0, -3, 3]), expected: TestExpected::IntVec(vec![0, 0, 9, 0, 0]), hidden: false },
                TestCase { input: TestInput::IntVec(vec![2, 5]), expected: TestExpected::IntVec(vec![5, 2]), hidden: false },
                TestCase { input: TestInput::IntVec(vec![0, 0, 4]), expected: TestExpected::IntVec(vec![0, 0, 0]), hidden: true },
                TestCase { input: TestInput::IntVec(vec![-2, -3, -4]), expected: TestExpected::IntVec(vec![12, 8, 6]), hidden: true },
                TestCase { input: TestInput::IntVec(vec![1, 1, 1, 1]), expected: TestExpected::IntVec(vec![1, 1, 1, 1]), hidden: true },
            ],
        },
        Exercise {
            id: "run-length-encode",
            title: "Run-Length Encode",
            difficulty: Difficulty::Medium,
            signature: ExerciseSignature::StrToStr,
            description: "Compress string `s` by replacing each maximal run of identical consecutive characters with the character followed by its decimal count (e.g. \"aaabbc\" -> \"a3b2c1\"). Empty input returns \"\".",
            constraints: "0 <= len(s) <= 10^4; ASCII letters.",
            tests: vec![
                TestCase { input: TestInput::Str("aaabbc".into()), expected: TestExpected::Str("a3b2c1".into()), hidden: false },
                TestCase { input: TestInput::Str("abcd".into()), expected: TestExpected::Str("a1b1c1d1".into()), hidden: false },
                TestCase { input: TestInput::Str("".into()), expected: TestExpected::Str("".into()), hidden: false },
                TestCase { input: TestInput::Str("zzzzzzzzzzzz".into()), expected: TestExpected::Str("z12".into()), hidden: true },
                TestCase { input: TestInput::Str("aAaA".into()), expected: TestExpected::Str("a1A1a1A1".into()), hidden: true },
                TestCase { input: TestInput::Str("x".into()), expected: TestExpected::Str("x1".into()), hidden: true },
                TestCase { input: TestInput::Str("aabbbba".into()), expected: TestExpected::Str("a2b4a1".into()), hidden: true },
            ],
        },
        Exercise {
            id: "subarray-sum-k",
            title: "Subarrays Summing to K",
            difficulty: Difficulty::Medium,
            signature: ExerciseSignature::IntVecIntToInt,
            description: "Return the total number of non-empty contiguous subarrays of `nums` whose elements sum to `k`.",
            constraints: "0 <= len(nums) <= 10^4; elements and k may be negative or zero.",
            tests: vec![
                TestCase { input: TestInput::IntVecAndInt(vec![1, 1, 1], 2), expected: TestExpected::Int(2), hidden: false },
                TestCase { input: TestInput::IntVecAndInt(vec![1, 2, 3], 3), expected: TestExpected::Int(2), hidden: false },
                TestCase { input: TestInput::IntVecAndInt(vec![1, -1, 0], 0), expected: TestExpected::Int(3), hidden: false },
                TestCase { input: TestInput::IntVecAndInt(vec![], 0), expected: TestExpected::Int(0), hidden: true },
                TestCase { input: TestInput::IntVecAndInt(vec![0, 0, 0], 0), expected: TestExpected::Int(6), hidden: true },
                TestCase { input: TestInput::IntVecAndInt(vec![3, 4, 7, 2, -3, 1, 4, 2], 7), expected: TestExpected::Int(4), hidden: true },
                TestCase { input: TestInput::IntVecAndInt(vec![-1, -1, 1], 0), expected: TestExpected::Int(1), hidden: true },
            ],
        },
        Exercise {
            id: "camel-to-snake",
            title: "CamelCase to snake_case",
            difficulty: Difficulty::Medium,
            signature: ExerciseSignature::StrToStr,
            description: "Convert a CamelCase or PascalCase identifier `s` into lowercase `snake_case`. Insert '_' before an uppercase letter that follows a lowercase/digit, or before the last uppercase letter of an acronym followed by a lowercase letter (e.g. \"parseHTMLDoc\" -> \"parse_html_doc\").",
            constraints: "0 <= len(s) <= 10^4; ASCII letters and digits.",
            tests: vec![
                TestCase { input: TestInput::Str("camelCase".into()), expected: TestExpected::Str("camel_case".into()), hidden: false },
                TestCase { input: TestInput::Str("parseHTMLDoc".into()), expected: TestExpected::Str("parse_html_doc".into()), hidden: false },
                TestCase { input: TestInput::Str("HTTPResponse2Code".into()), expected: TestExpected::Str("http_response2_code".into()), hidden: false },
                TestCase { input: TestInput::Str("".into()), expected: TestExpected::Str("".into()), hidden: true },
                TestCase { input: TestInput::Str("already".into()), expected: TestExpected::Str("already".into()), hidden: true },
                TestCase { input: TestInput::Str("JSON".into()), expected: TestExpected::Str("json".into()), hidden: true },
                TestCase { input: TestInput::Str("SimpleTestCase".into()), expected: TestExpected::Str("simple_test_case".into()), hidden: true },
            ],
        },

        // ════════════════════════════════════════════════════════════════════
        // HARD (6)
        // ════════════════════════════════════════════════════════════════════
        Exercise {
            id: "coin-change-min",
            title: "Minimum Coin Change",
            difficulty: Difficulty::Hard,
            signature: ExerciseSignature::IntVecIntToInt,
            description: "Given coin denominations `nums` (unlimited supply of each positive coin) and a target amount `k`, return the fewest coins needed to make `k`, or `-1` if it cannot be formed.",
            constraints: "1 <= len(nums) <= 20; 0 <= k <= 10^4.",
            tests: vec![
                TestCase { input: TestInput::IntVecAndInt(vec![1, 2, 5], 11), expected: TestExpected::Int(3), hidden: false },
                TestCase { input: TestInput::IntVecAndInt(vec![2], 3), expected: TestExpected::Int(-1), hidden: false },
                TestCase { input: TestInput::IntVecAndInt(vec![1, 3, 4], 6), expected: TestExpected::Int(2), hidden: false },
                TestCase { input: TestInput::IntVecAndInt(vec![1], 0), expected: TestExpected::Int(0), hidden: true },
                TestCase { input: TestInput::IntVecAndInt(vec![5, 10], 7), expected: TestExpected::Int(-1), hidden: true },
                TestCase { input: TestInput::IntVecAndInt(vec![186, 419, 83, 408], 6249), expected: TestExpected::Int(20), hidden: true },
                TestCase { input: TestInput::IntVecAndInt(vec![2, 5, 10, 1], 27), expected: TestExpected::Int(4), hidden: true },
            ],
        },
        Exercise {
            id: "trapping-rain-water",
            title: "Trapping Rain Water",
            difficulty: Difficulty::Hard,
            signature: ExerciseSignature::IntVecToInt,
            description: "Given non-negative bar heights `nums` of width 1, compute the total volume of rainwater trapped between the bars after raining.",
            constraints: "0 <= len(nums) <= 10^5; 0 <= nums[i] <= 10^5.",
            tests: vec![
                TestCase { input: TestInput::IntVec(vec![0, 1, 0, 2, 1, 0, 1, 3, 2, 1, 2, 1]), expected: TestExpected::Int(6), hidden: false },
                TestCase { input: TestInput::IntVec(vec![4, 2, 0, 3, 2, 5]), expected: TestExpected::Int(9), hidden: false },
                TestCase { input: TestInput::IntVec(vec![1, 2, 3, 4]), expected: TestExpected::Int(0), hidden: false },
                TestCase { input: TestInput::IntVec(vec![]), expected: TestExpected::Int(0), hidden: true },
                TestCase { input: TestInput::IntVec(vec![5]), expected: TestExpected::Int(0), hidden: true },
                TestCase { input: TestInput::IntVec(vec![5, 0, 5]), expected: TestExpected::Int(5), hidden: true },
                TestCase { input: TestInput::IntVec(vec![3, 0, 1, 3, 0, 5]), expected: TestExpected::Int(8), hidden: true },
            ],
        },
        Exercise {
            id: "next-greater-circular",
            title: "Next Greater (Circular)",
            difficulty: Difficulty::Hard,
            signature: ExerciseSignature::IntVecToIntVec,
            description: "For each index i in circular array `nums`, find the first strictly greater value encountered moving forward (wrapping around from the end to index 0). Use `-1` if no greater value exists.",
            constraints: "0 <= len(nums) <= 10^4.",
            tests: vec![
                TestCase { input: TestInput::IntVec(vec![1, 2, 1]), expected: TestExpected::IntVec(vec![2, -1, 2]), hidden: false },
                TestCase { input: TestInput::IntVec(vec![1, 2, 3, 4, 3]), expected: TestExpected::IntVec(vec![2, 3, 4, -1, 4]), hidden: false },
                TestCase { input: TestInput::IntVec(vec![5, 4, 3, 2, 1]), expected: TestExpected::IntVec(vec![-1, 5, 5, 5, 5]), hidden: false },
                TestCase { input: TestInput::IntVec(vec![]), expected: TestExpected::IntVec(vec![]), hidden: true },
                TestCase { input: TestInput::IntVec(vec![7]), expected: TestExpected::IntVec(vec![-1]), hidden: true },
                TestCase { input: TestInput::IntVec(vec![3, 3, 3]), expected: TestExpected::IntVec(vec![-1, -1, -1]), hidden: true },
                TestCase { input: TestInput::IntVec(vec![-1, -2, 0]), expected: TestExpected::IntVec(vec![0, 0, -1]), hidden: true },
            ],
        },
        Exercise {
            id: "longest-valid-parens",
            title: "Longest Valid Parentheses",
            difficulty: Difficulty::Hard,
            signature: ExerciseSignature::StrToInt,
            description: "Given a string `s` consisting only of '(' and ')', return the length of the longest well-formed (balanced) parentheses contiguous substring.",
            constraints: "0 <= len(s) <= 10^5.",
            tests: vec![
                TestCase { input: TestInput::Str("(()".into()), expected: TestExpected::Int(2), hidden: false },
                TestCase { input: TestInput::Str(")()())".into()), expected: TestExpected::Int(4), hidden: false },
                TestCase { input: TestInput::Str("()(()".into()), expected: TestExpected::Int(2), hidden: false },
                TestCase { input: TestInput::Str("".into()), expected: TestExpected::Int(0), hidden: true },
                TestCase { input: TestInput::Str("(()())".into()), expected: TestExpected::Int(6), hidden: true },
                TestCase { input: TestInput::Str(")(((((()())()()))()(()))(".into()), expected: TestExpected::Int(22), hidden: true },
                TestCase { input: TestInput::Str("))))(((".into()), expected: TestExpected::Int(0), hidden: true },
            ],
        },
        Exercise {
            id: "is-wildcard-match",
            title: "Wildcard Pattern Match",
            difficulty: Difficulty::Hard,
            signature: ExerciseSignature::TwoStrsToBool,
            description: "Return true if the entire text `a` matches pattern `b`, where '?' matches any single character and '*' matches any sequence of zero or more characters.",
            constraints: "0 <= len(a), len(b) <= 500.",
            tests: vec![
                TestCase { input: TestInput::TwoStrs("aa".into(), "a".into()), expected: TestExpected::Bool(false), hidden: false },
                TestCase { input: TestInput::TwoStrs("aa".into(), "*".into()), expected: TestExpected::Bool(true), hidden: false },
                TestCase { input: TestInput::TwoStrs("adceb".into(), "*a*b".into()), expected: TestExpected::Bool(true), hidden: false },
                TestCase { input: TestInput::TwoStrs("acdcb".into(), "a*c?b".into()), expected: TestExpected::Bool(false), hidden: true },
                TestCase { input: TestInput::TwoStrs("".into(), "***".into()), expected: TestExpected::Bool(true), hidden: true },
                TestCase { input: TestInput::TwoStrs("".into(), "?".into()), expected: TestExpected::Bool(false), hidden: true },
                TestCase { input: TestInput::TwoStrs("abefcdgiescdfimde".into(), "ab*cd?i*de".into()), expected: TestExpected::Bool(true), hidden: true },
                TestCase { input: TestInput::TwoStrs("aaaaaaaaaaaaaaaaaaaaaaaa".into(), "*a*a*a*a*a*b".into()), expected: TestExpected::Bool(false), hidden: true },
            ],
        },
        Exercise {
            id: "count-ways-stairs-k",
            title: "Tribonacci Stairs",
            difficulty: Difficulty::Hard,
            signature: ExerciseSignature::IntToInt,
            description: "You are climbing a staircase with `n` steps and can take 1, 2, or 3 steps at a time. Return the exact number of distinct ways to reach step `n` (with 1 way for n = 0).",
            constraints: "0 <= n <= 50; fits in signed 64-bit integer.",
            tests: vec![
                TestCase { input: TestInput::Int(0), expected: TestExpected::Int(1), hidden: false },
                TestCase { input: TestInput::Int(4), expected: TestExpected::Int(7), hidden: false },
                TestCase { input: TestInput::Int(10), expected: TestExpected::Int(274), hidden: false },
                TestCase { input: TestInput::Int(1), expected: TestExpected::Int(1), hidden: true },
                TestCase { input: TestInput::Int(2), expected: TestExpected::Int(2), hidden: true },
                TestCase { input: TestInput::Int(3), expected: TestExpected::Int(4), hidden: true },
                TestCase { input: TestInput::Int(30), expected: TestExpected::Int(53798080), hidden: true },
                TestCase { input: TestInput::Int(45), expected: TestExpected::Int(501774317241), hidden: true },
            ],
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn catalog_has_unique_ids_valid_cases_and_starters_for_all_languages() {
        let all = all_exercises();
        assert_eq!(all.len(), 18);
        let mut ids = HashSet::new();
        for diff in Difficulty::ALL {
            let pool = exercises_for_difficulty(*diff);
            assert_eq!(pool.len(), 6, "expected 6 exercises for {:?}", diff);
        }
        for ex in &all {
            assert!(ids.insert(ex.id), "duplicate exercise id: {}", ex.id);
            assert!(ex.tests.len() >= 6, "too few tests for {}", ex.id);
            assert!(
                ex.tests.iter().any(|t| !t.hidden) && ex.tests.iter().any(|t| t.hidden),
                "exercise {} needs both visible and hidden tests",
                ex.id
            );
            for &lang in PracticeLanguage::ALL {
                let code = starter_code(ex, lang);
                assert!(
                    code.contains("solve"),
                    "starter code missing `solve` for {} in {:?}",
                    ex.id,
                    lang
                );
            }
        }
    }

    #[test]
    fn daily_selection_is_deterministic_and_shared_across_languages() {
        let d1 = LocalDate::from_ymd(2026, 11, 26).unwrap();
        let a = daily_exercise(d1, Difficulty::Easy);
        let b = daily_exercise(d1, Difficulty::Easy);
        assert_eq!(a.id, b.id);

        // C and Zig must both be first-class languages in PracticeLanguage::ALL.
        assert!(PracticeLanguage::ALL.contains(&PracticeLanguage::C));
        assert!(PracticeLanguage::ALL.contains(&PracticeLanguage::Zig));

        // Across a month of dates, daily selection rotates across multiple exercises.
        let mut seen = HashSet::new();
        let mut cur = d1;
        for _ in 0..30 {
            let ex = daily_exercise(cur, Difficulty::Medium);
            seen.insert(ex.id);
            cur = cur.next_day();
        }
        assert!(seen.len() >= 4, "expected rotation across pool, saw {:?}", seen);
    }
}
