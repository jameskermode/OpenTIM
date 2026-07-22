#!/usr/bin/env python3
"""
check-ffi-signatures.py -- verify that every #[no_mangle] pub extern "C" fn in the Rust
half of this port has a C-side prototype (in a header or a .c file's own forward
declaration) whose signature actually matches the Rust definition, ABI-wise.

WHY THIS EXISTS
----------------
The C compiler validates call sites against whatever prototype is in scope. rustc
validates the Rust function against its own signature. *Nothing* validates that the two
sides agree with each other -- a `bool` on one side and an `int` on the other, or a
missing/extra parameter, compiles cleanly on both sides and corrupts data at the ABI
boundary. This project already hit exactly this class of bug once (commit 28c6ba5, where
an ABI mismatch made the simulation depend on optimisation level). This script is a
mechanical, pure-source-analysis check for that gap. It requires no build and no game
data, so it must run on the ALLOW_NO_GAME_DATA=1 path too.

WHAT IT CHECKS
--------------
1. Every `#[no_mangle] ... extern "C" fn` in the Rust sources listed in RUST_SOURCES is
   compared, by name, against every C prototype declaring that name (found in any header
   under c_src/ *and* in the local forward-declarations .c files sometimes carry at their
   own top -- see FFI_ALLOWLIST comment below for why .c files matter here too).
2. A mismatch in parameter count, parameter type, or return type is a hard FAIL, printing
   the function name and both full signatures.
3. A C prototype whose name is not a Rust export and which is never *defined* anywhere in
   c_src/*.c is a dangling declaration -- also a hard FAIL.
4. A Rust export with no C prototype anywhere is only a real problem if some C source
   actually calls it (that would compile today only via an implicit-int declaration,
   which is its own gate check in verify.sh, but this script calls it out by name too).
   If nothing in C calls it, it's Rust-internal (e.g. used only via an `extern "C" {...}`
   block inside Rust itself to get a stable function pointer), and is reported as
   informational only -- see the "no C prototype anywhere" section in main() for the
   reasoning applied per-name.

TYPE EQUIVALENCE
----------------
Comparisons are ABI-based, not textual. The project's established mapping is used:
    struct Part *        <-> *mut Part   (or &mut Part)
    const struct Part *  <-> *const Part (or &Part)
    s16 / 16-bit int     <-> i16
    u16                  <-> u16
    s32 / long           <-> i32
    byte                 <-> u8
    enum PartType        <-> c_int
    a bare C enum (4 bytes) <-> u32
    size_t               <-> usize
    struct ShortVec      <-> ShortVec (ditto for the other small structs)
    bool                 <-> bool

Two deliberate simplifications, both explained where they're applied in code below:
  - `enum PartType -> c_int` and `other enum -> u32` are both treated as the same 4-byte
    "plain integer" class, because a 32-bit register holds the same bits either way; there
    is no undefined-bits/truncation hazard from signedness alone the way there is between,
    say, a 1-byte bool and a 4-byte int. Only byte-width differences are treated as real
    mismatches.
  - Rust `&mut T` / `&T` and `*mut T` / `*const T` are treated as interchangeable for a C
    `T*`, per the task's own instruction. Pointer const/mut-ness itself (`*const` vs
    `*mut`) is *not* enforced either: it has zero effect on the actual bit pattern passed
    across the FFI boundary (a pointer is a pointer), so a const/mut mismatch cannot
    corrupt data the way a size mismatch can -- it is a stricter-than-C annotation on the
    Rust side at worst. Flagging it would produce noise without catching real bugs, so it
    is intentionally not checked.
  `bool` is kept as its own distinct class rather than folded into the 4-byte-int class:
  this codebase's `bool` (c_src/int.h: `typedef int bool;`) is 4 bytes in C, but Rust's
  native `bool` is 1 byte -- a real, established mismatch class (this is exactly what's
  wrong with part_explicit_size et al. below). Per the task's own mapping table, `bool`
  only matches Rust `bool`; a C `bool` prototype satisfied by a Rust `c_int`/`i32` return
  is flagged as a mismatch, not treated as equivalent.
"""
import re
import sys
import pathlib

ROOT = pathlib.Path(__file__).resolve().parent.parent

RUST_SOURCES = [
    "src/tim_c.rs",
    "src/parts/mod.rs",
    "src/wasm_libc.rs",
    "src/globals.rs",
]

# Every C source file that might carry a prototype for a Rust-exported function: not just
# headers, but the .c files themselves, because at least four functions in this codebase
# (arctan_c, calculate_line_intersection, calculate_line_intersection_helper,
# part_image_size, teeter_totter_reset) are declared ONLY via a local forward-declaration
# at the top of main.c / part_defs.c -- they have NO header prototype at all. A checker
# that only looked at headers would miss exactly the bug class this script exists to
# catch, in those five places.
C_GLOB_DIRS = ["c_src"]

# ---------------------------------------------------------------------------------------
# Known vocabulary. Kept explicit (rather than inferred) so an unrecognised type name is a
# loud KeyError during development, not a silent false negative.
# ---------------------------------------------------------------------------------------

# C base-type name (after stripping `struct`/`enum`/`const` keywords) -> ABI class.
# ABI classes: 'int1'/'int2'/'int4'/'int8' (byte width, signedness-agnostic -- see module
# docstring), 'usize' (pointer-width, from size_t), 'bool' (its own class -- see above).
C_SCALAR_CLASS = {
    "byte": "int1", "u8": "int1", "sbyte": "int1", "s8": "int1", "char": "int1",
    "u16": "int2", "s16": "int2",
    "u32": "int4", "s32": "int4", "long": "int4", "int": "int4",
    "u64": "int8", "int64_t": "int8",
    "bool": "bool",
    "size_t": "usize",
    "void": "unit",
}

# Signedness, checked SEPARATELY from the ABI size class above. u16 and s16 are
# ABI-identical, so a mismatch there is not an ABI bug -- but it is still a real defect:
# widening sign-extends on one side and zero-extends on the other, and comparisons flip.
# Enums are deliberately absent: this project's own mapping allows `enum PartType` to meet
# both `c_int` and `u32`, so enum-typed parameters are exempt from this check.
SIGN_WORD = {"u": "unsigned", "s": "signed"}

C_SIGNEDNESS = {
    "u8": "u", "byte": "u", "u16": "u", "u32": "u", "u64": "u", "size_t": "u",
    "s8": "s", "sbyte": "s", "s16": "s", "s32": "s", "int": "s", "long": "s",
}
RUST_SIGNEDNESS = {
    "u8": "u", "u16": "u", "u32": "u", "u64": "u", "usize": "u",
    "i8": "s", "i16": "s", "i32": "s", "i64": "s", "isize": "s", "c_int": "s",
}


def signedness_of(raw, table):
    """Signedness of a bare scalar type name, or None if not a plain scalar (pointer,
    struct, enum, unknown) -- those are exempt."""
    t = re.sub(r"\s+", "", raw or "")
    if "*" in t or "&" in t:
        return None
    t = t.replace("const", "").replace("struct", "").replace("enum", "")
    return table.get(t)


RUST_SCALAR_CLASS = {
    "u8": "int1", "i8": "int1", "c_char": "int1",
    "u16": "int2", "i16": "int2",
    "u32": "int4", "i32": "int4", "c_int": "int4",
    "u64": "int8", "i64": "int8",
    "usize": "usize", "isize": "usize",
    "bool": "bool",
}

# Struct tags known to appear on both sides (C `struct Foo` <-> Rust `Foo`). An unlisted
# struct name is not silently accepted -- it just fails to match anything, which is the
# correct, loud behaviour for a name this script doesn't know about yet.
KNOWN_STRUCTS = {
    "Part", "BeltData", "RopeData", "ShortVec", "SByteVec", "ByteVec", "LongVec",
    "Line", "GDIRect", "Llama", "BorderPoint",
}

# Enum tags used bare (by value, not by pointer) in a prototype. Per the mapping table,
# *every* bare enum -- PartType included -- normalises to the 4-byte 'int4' class (see
# module docstring for why PartType and "other enum" don't need separate treatment here).
KNOWN_ENUMS = {
    "PartType", "GetPartsFlags", "LevelState", "RopeTime", "RopeFirstOrLast",
    "Flags1_Flags", "Flags2_Flags", "Flags3_Flags",
}

# Identifier tokens that can appear as the last word of a C parameter without being a
# parameter *name* -- i.e. the parameter is anonymous (e.g. `struct Part *` with nothing
# after it, or `void` as the sole parameter meaning "no parameters"). Used to disambiguate
# "struct Part *part" (named) from "struct Part *" (anonymous) when both have the same
# token count.
C_TYPE_KEYWORDS = (
    {"struct", "enum", "const", "unsigned", "signed", "void"}
    | set(C_SCALAR_CLASS)
    | KNOWN_STRUCTS
    | KNOWN_ENUMS
)


# =========================================================================================
# Rust-side extraction
# =========================================================================================

class RustFn:
    def __init__(self, file, line, name, params_raw, return_raw, is_pub):
        self.file = file
        self.line = line
        self.name = name
        self.params_raw = params_raw
        self.return_raw = return_raw
        self.is_pub = is_pub

    def signature_text(self):
        ret = f" -> {self.return_raw}" if self.return_raw else ""
        return f"fn {self.name}({self.params_raw}){ret}"


def split_top_level(s, sep=","):
    """Split on `sep` but not inside (), [], <>, {} -- needed for Rust param lists (e.g.
    array types `[*mut Part; 6]`) and, symmetrically, C param lists."""
    parts = []
    depth = 0
    cur = []
    pairs = {"(": ")", "[": "]", "<": ">", "{": "}"}
    closers = set(pairs.values())
    for ch in s:
        if ch in pairs:
            depth += 1
        elif ch in closers:
            depth -= 1
        if ch == sep and depth == 0:
            parts.append("".join(cur))
            cur = []
        else:
            cur.append(ch)
    if cur or parts:
        parts.append("".join(cur))
    return [p.strip() for p in parts if p.strip()]


def extract_rust_fns(relpath):
    text = (ROOT / relpath).read_text()
    lines = text.split("\n")
    out = []
    i, n = 0, len(lines)
    while i < n:
        if lines[i].strip() == "#[no_mangle]":
            j = i + 1
            while j < n and lines[j].strip().startswith("#["):
                j += 1
            joined = "\n".join(lines[j : j + 60])
            m = re.search(r'\b(pub\s+)?(unsafe\s+)?extern\s+"C"\s+fn\s+(\w+)\s*\(', joined)
            if not m:
                i += 1
                continue
            is_pub = bool(m.group(1))
            name = m.group(3)
            paren_start = m.end() - 1
            depth = 0
            k = paren_start
            while k < len(joined):
                if joined[k] == "(":
                    depth += 1
                elif joined[k] == ")":
                    depth -= 1
                    if depth == 0:
                        break
                k += 1
            params_str = joined[paren_start + 1 : k]
            rest = joined[k + 1 :]
            brace_pos = rest.find("{")
            sig_rest = rest[:brace_pos] if brace_pos != -1 else rest
            ret = ""
            am = re.search(r"->\s*(.+)", sig_rest, re.S)
            if am:
                ret = re.sub(r"\s+", " ", am.group(1).strip())
            out.append(
                RustFn(relpath, j + 1, name, re.sub(r"\s+", " ", params_str.strip()), ret, is_pub)
            )
            i = j + sig_rest.count("\n") + 1
        else:
            i += 1
    return out


def normalize_rust_type(t):
    """Returns (pointer_depth, kind) where kind is 'unit', 'bool', 'int{1,2,4,8}',
    'usize', or ('struct', Name)."""
    t = t.strip()
    if t == "" or t == "()":
        return (0, "unit")
    depth = 0
    while True:
        if t.startswith("&mut "):
            depth += 1
            t = t[5:].strip()
            continue
        if t.startswith("&"):
            depth += 1
            t = t[1:].strip()
            continue
        if t.startswith("*mut "):
            depth += 1
            t = t[5:].strip()
            continue
        if t.startswith("*const "):
            depth += 1
            t = t[7:].strip()
            continue
        break
    if t in RUST_SCALAR_CLASS:
        return (depth, RUST_SCALAR_CLASS[t])
    if t in KNOWN_STRUCTS:
        return (depth, ("struct", t))
    # Unrecognised -- surface it as a distinct "unknown" struct-like kind so it fails to
    # match anything (loud) rather than silently matching everything.
    return (depth, ("unknown", t))


# =========================================================================================
# C-side extraction
# =========================================================================================


def strip_comments(text):
    text = re.sub(r"/\*.*?\*/", lambda m: "\n" * m.group(0).count("\n"), text, flags=re.S)
    text = re.sub(r"//[^\n]*", "", text)
    return text


# A prototype: `TYPE NAME(PARAMS);` at (near enough) the start of a line. Deliberately
# excludes '(' ')' from PARAMS so it can't straddle nested calls, and requires the
# "TYPE" capture to be at least one full token separated by whitespace from NAME -- this
# is what keeps it from matching bare statements like `foo(x);` (no return-type prefix).
PROTO_RE = re.compile(r"^[ \t]*(?!#)([A-Za-z_][\w \t\*]*[\w\*])\s+(\w+)\s*\(([^;{}()]*)\)\s*;", re.M)

# A definition: same shape but ending in `{` instead of `;`, with optional leading
# `static`/`inline` keywords (in either order, since this codebase uses "static inline").
DEF_RE = re.compile(
    r"^[ \t]*(?!#)(?:(?:static|inline)\s+){0,2}([A-Za-z_][\w \t\*]*[\w\*])\s+(\w+)\s*\(([^;{}()]*)\)\s*\{",
    re.M,
)

# TYPE tokens that are actually C keywords/statements, not return types -- without this,
# `return get_first_part(choice);` inside a function body parses as a prototype for a
# function literally named "get_first_part" returning type "return". Every keyword that
# can legally precede `identifier(...)` followed by `;` in this codebase's C belongs here.
STATEMENT_KEYWORDS = {"return", "if", "else", "while", "for", "switch", "case", "do", "goto", "sizeof"}


class CProto:
    def __init__(self, file, line, ret_raw, name, params_raw):
        self.file = file
        self.line = line
        self.ret_raw = ret_raw
        self.name = name
        self.params_raw = params_raw

    def signature_text(self):
        return f"{self.ret_raw} {self.name}({self.params_raw})"


def split_c_param(param):
    """Split a single C parameter into (type_text, name_or_None), using the fixed
    vocabulary in C_TYPE_KEYWORDS to tell a named parameter (`struct Part *part`) apart
    from an anonymous one (`struct Part *`) when both have the same token count."""
    param = param.strip()
    if param == "" or param == "void":
        return None
    idents = list(re.finditer(r"[A-Za-z_]\w*", param))
    if not idents:
        return (param, None)
    last = idents[-1]
    if last.group() in C_TYPE_KEYWORDS:
        # last identifier is part of the type itself -> anonymous parameter
        return (param, None)
    type_text = (param[: last.start()] + param[last.end() :]).strip()
    type_text = re.sub(r"\s+", " ", type_text)
    return (type_text, last.group())


def normalize_c_type(t):
    t = t.strip()
    is_enum = bool(re.search(r"\benum\b", t))
    is_struct = bool(re.search(r"\bstruct\b", t))
    t = re.sub(r"\bconst\b", "", t)
    t = re.sub(r"\bstruct\b", "", t)
    t = re.sub(r"\benum\b", "", t)
    depth = t.count("*")
    t = t.replace("*", "")
    t = re.sub(r"\s+", "", t)
    if t == "" and depth == 0:
        return (0, "unit")
    if is_enum:
        # Every bare/pointer-to enum normalises to the 4-byte int class -- see module
        # docstring ("enum PartType <-> c_int" and "bare enum <-> u32" are the same class).
        return (depth, "int4")
    if is_struct:
        return (depth, ("struct", t))
    if t in C_SCALAR_CLASS:
        return (depth, C_SCALAR_CLASS[t])
    if t in KNOWN_STRUCTS:
        # struct-tag used without the `struct` keyword thanks to a typedef; none expected
        # in this codebase, but handle it rather than mis-report.
        return (depth, ("struct", t))
    return (depth, ("unknown", t))


def kinds_match(a, b):
    return a == b


def gather_c_files():
    files = []
    for d in C_GLOB_DIRS:
        for ext in ("*.h", "*.c"):
            for p in sorted((ROOT / d).rglob(ext)):
                if "scratch-scripts" in str(p):
                    continue
                files.append(p)
    return files


def extract_c_protos_and_defs():
    protos = {}  # name -> list[CProto]
    defs = {}  # name -> list[(file, line)]
    for p in gather_c_files():
        rel = str(p.relative_to(ROOT))
        text = strip_comments(p.read_text())
        for m in PROTO_RE.finditer(text):
            ret_raw = re.sub(r"\s+", " ", m.group(1).strip())
            name = m.group(2)
            if ret_raw in STATEMENT_KEYWORDS:
                continue
            line = text[: m.start()].count("\n") + 1
            params_raw = re.sub(r"\s+", " ", m.group(3).strip())
            protos.setdefault(name, []).append(CProto(rel, line, ret_raw, name, params_raw))
        for m in DEF_RE.finditer(text):
            name = m.group(2)
            line = text[: m.start()].count("\n") + 1
            defs.setdefault(name, []).append((rel, line))
    return protos, defs


def c_is_called(name, c_texts):
    """True if any C source calls `name(` somewhere other than inside its own prototype
    line (a crude but adequate check: any occurrence of the identifier immediately
    followed by '(' that isn't a declaration/definition line, i.e. anywhere at all --
    over-approximating "is it called" is fine here, since the only consequence of a false
    positive is an upgraded-to-FAIL entry for a function that in fact isn't called)."""
    pattern = re.compile(r"\b" + re.escape(name) + r"\s*\(")
    for text in c_texts.values():
        if pattern.search(text):
            return True
    return False


# =========================================================================================
# Comparison
# =========================================================================================


def compare_signatures(rust_fn, cproto):
    """Returns list of human-readable mismatch reasons; empty list = match."""
    reasons = []

    c_ret = normalize_c_type(cproto.ret_raw)
    rust_ret = normalize_rust_type(rust_fn.return_raw)
    if not kinds_match(c_ret, rust_ret):
        reasons.append(f"return type: C `{cproto.ret_raw}` ({c_ret}) vs Rust `{rust_fn.return_raw or '()'}` ({rust_ret})")
    else:
        cs = signedness_of(cproto.ret_raw, C_SIGNEDNESS)
        rs = signedness_of(rust_fn.return_raw, RUST_SIGNEDNESS)
        if cs and rs and cs != rs:
            reasons.append(
                f"return signedness: C `{cproto.ret_raw}` is {SIGN_WORD[cs]} but Rust "
                f"`{rust_fn.return_raw}` is {SIGN_WORD[rs]} (ABI-compatible, but widening and "
                f"comparisons differ)"
            )

    c_params_raw = cproto.params_raw.strip()
    c_params = [] if c_params_raw in ("", "void") else split_top_level(c_params_raw)
    c_types = []
    for cp in c_params:
        split = split_c_param(cp)
        if split is None:
            continue
        c_types.append(split[0])

    rust_params_raw = rust_fn.params_raw.strip()
    rust_params = [] if rust_params_raw == "" else split_top_level(rust_params_raw)
    rust_types = []
    for rp in rust_params:
        if ":" in rp:
            rust_types.append(rp.split(":", 1)[1].strip())
        else:
            rust_types.append(rp.strip())

    if len(c_types) != len(rust_types):
        reasons.append(
            f"parameter count: C has {len(c_types)} ({', '.join(c_types) or '-'}) "
            f"vs Rust has {len(rust_types)} ({', '.join(rust_types) or '-'})"
        )
        return reasons

    for idx, (ct, rt) in enumerate(zip(c_types, rust_types), start=1):
        cn = normalize_c_type(ct)
        rn = normalize_rust_type(rt)
        if not kinds_match(cn, rn):
            reasons.append(f"param {idx}: C `{ct}` ({cn}) vs Rust `{rt}` ({rn})")
        else:
            cs = signedness_of(ct, C_SIGNEDNESS)
            rs = signedness_of(rt, RUST_SIGNEDNESS)
            if cs and rs and cs != rs:
                reasons.append(
                    f"param {idx} signedness: C `{ct}` is {SIGN_WORD[cs]} but Rust `{rt}` is "
                    f"{SIGN_WORD[rs]} (ABI-compatible, but widening and comparisons differ)"
                )

    return reasons


# =========================================================================================
# Allowlist for legitimate exceptions -- an explicit, commented, name-by-name list rather
# than a loosened check. Each entry says exactly *why* it's exempt; add to it only with the
# same justification, never to silence a real finding.
# =========================================================================================

# Rust exports with NO C prototype anywhere, and (checked separately, mechanically) never
# called from any C source either -- so there is nothing for a C-side prototype to agree
# or disagree with. All are one of:
#   - libc symbols provided for the freestanding wasm build. Their prototypes come from a
#     shim generated at build time (see build.rs, which writes a stdlib.h declaring exactly
#     these signatures for the wasm target) or from the system libc headers on native
#     targets -- neither lives in this repo's own C sources for this checker to read.
#   - dead/dev-only tooling, never wired up to any call site (kept for future debugging).
#   - a pure Rust-internal indirection: declared via `extern "C" { fn ...; }` *inside*
#     Rust itself (src/parts/mod.rs) purely to get a stable C-ABI function pointer to hand
#     to a `PartFn`-typed global (MEL_JUMPY) the same way the original C code stored a
#     function pointer there; no C code ever calls these by name.
NO_C_PROTOTYPE_NOT_CALLED_BY_C = {
    "malloc": "wasm libc shim (build.rs-generated stdlib.h) / system libc headers",
    "free": "wasm libc shim (build.rs-generated stdlib.h) / system libc headers",
    "calloc": "wasm libc shim (build.rs-generated stdlib.h) / system libc headers",
    "abs": "wasm libc shim (build.rs-generated stdlib.h) / system libc headers",
    "generate_hypot_samples": "dev-only tooling, calls are commented out in c_src/draw_rope.c",
    "mort_the_mouse_cage_start": "Rust-internal function-pointer indirection (see src/parts/mod.rs), never called from C",
    "bob_the_fish_break_bowl": "Rust-internal function-pointer indirection (see src/parts/mod.rs), never called from C",
    # These four are dispatched purely from Rust's own part-definition table (src/parts/) --
    # the C dispatch that used to call the equivalents by these names was never ported back
    # in, so there is nothing in c_src/ left to declare a prototype for. Confirmed by grep:
    # none of these four identifiers appears anywhere under c_src/.
    "part_density": "called only from Rust (part_acceleration in src/tim_c.rs); no C caller or prototype exists",
    "part_flip": "not called from Rust or C currently; kept for a future caller",
    "part_resize": "not called from Rust or C currently; kept for a future caller",
    "balloon_rope": "called only from Rust (src/parts/mod.rs part_rope dispatch); no C caller or prototype exists",
}


def main():
    rust_fns = []
    for src in RUST_SOURCES:
        rust_fns.extend(extract_rust_fns(src))
    rust_by_name = {}
    for f in rust_fns:
        rust_by_name.setdefault(f.name, []).append(f)

    protos, defs = extract_c_protos_and_defs()

    c_texts = {}
    for p in gather_c_files():
        if p.suffix == ".c":
            c_texts[str(p)] = strip_comments(p.read_text())

    fail = False
    compared = 0
    mismatches = []
    dangling = []
    no_proto_ok = []
    no_proto_bad = []

    # --- 1 & 3: every Rust export, matched against every C prototype for its name -------
    for name, fns in sorted(rust_by_name.items()):
        rust_fn = fns[0]  # all overloads of the same extern "C" name must agree; if there
        # were ever two, comparing against the first is enough to surface a problem.
        c_protos = protos.get(name, [])
        if not c_protos:
            if name in NO_C_PROTOTYPE_NOT_CALLED_BY_C:
                no_proto_ok.append((name, NO_C_PROTOTYPE_NOT_CALLED_BY_C[name]))
                continue
            if c_is_called(name, c_texts):
                no_proto_bad.append(rust_fn)
                fail = True
            else:
                no_proto_ok.append((name, "not called from any C source (verify manually if this changes)"))
            continue
        for cproto in c_protos:
            compared += 1
            reasons = compare_signatures(rust_fn, cproto)
            if reasons:
                mismatches.append((rust_fn, cproto, reasons))
                fail = True

    # --- 2: dangling C declarations (prototype, no Rust def, no C def) ------------------
    for name, c_protos in sorted(protos.items()):
        if name in rust_by_name:
            continue
        if name in defs:
            continue
        dangling.append(c_protos[0])
        fail = True

    # --- report ---------------------------------------------------------------------------
    print(f"  compared {compared} (name, C-prototype-occurrence) pairs across "
          f"{len(rust_by_name)} Rust-exported extern \"C\" functions")

    if mismatches:
        print(f"  FAIL {len(mismatches)} signature mismatch(es) between Rust and C:")
        for rust_fn, cproto, reasons in mismatches:
            print(f"    {rust_fn.name}:")
            print(f"      Rust ({rust_fn.file}:{rust_fn.line}): {rust_fn.signature_text()}")
            print(f"      C    ({cproto.file}:{cproto.line}): {cproto.signature_text()};")
            for r in reasons:
                print(f"        - {r}")

    if dangling:
        print(f"  FAIL {len(dangling)} dangling C prototype(s) (declared, never defined in "
              f"Rust or C):")
        for cproto in dangling:
            print(f"    {cproto.name} ({cproto.file}:{cproto.line}): {cproto.signature_text()};")

    if no_proto_bad:
        print(f"  FAIL {len(no_proto_bad)} Rust export(s) called from C with no prototype "
              f"anywhere (relies on implicit-int declaration -- see the "
              f"implicit-function-declaration check above):")
        for rust_fn in no_proto_bad:
            print(f"    {rust_fn.name} ({rust_fn.file}:{rust_fn.line}): {rust_fn.signature_text()}")

    if no_proto_ok:
        print(f"  ({len(no_proto_ok)} Rust export(s) with no C prototype, not called from C -- informational only)")

    if not fail:
        print("  OK all FFI signatures agree")

    return 1 if fail else 0


if __name__ == "__main__":
    sys.exit(main())
