# Ghidra setup against the reference binary

This document records how a Ghidra project was created against the reference `TEMIM.EXE`,
what worked and what didn't, and the result of validating the `TIMWIN: SSSS:OOOO` address
convention used throughout `c_src/` and `src/tim_c.rs`.

No project source was changed by this work. No copyrighted binary or Ghidra project data is
committed to the repository — everything lives under the git-ignored `game-data/` directory
(`game-data/ghidra-project/`, plus a throwaway `game-data/pyghidra-venv/` used only for the
scripting workaround described below) and is not part of this commit.

## Tool versions

- **Ghidra 12.1.2**, installed from the Homebrew **core formula** `ghidra` (`brew install
  ghidra`), *not* the cask. `brew info ghidra` shows a bottled formula exists, so the
  Gatekeeper/ad-hoc-signing problem seen earlier in this project with `dosbox-x-app` did not
  arise here — nothing needed to be exempted from `spctl`.
- JDK: **Eclipse Temurin was not actually needed.** The `ghidra` formula pulls in
  **`openjdk@21` (21.0.12)** as a required dependency, which already satisfies Ghidra's JDK
  17+ requirement. Verified with:
  ```
  $ /opt/homebrew/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home/bin/java -version
  openjdk version "21.0.12" 2026-07-21
  ```
  `openjdk@21` is keg-only and not linked into `/usr/libexec/java_home`, so `JAVA_HOME` must
  be set explicitly to that path for `analyzeHeadless` to find a JRE.
- Ghidra install path: `/opt/homebrew/Cellar/ghidra/12.1.2/libexec` (this is
  `$GHIDRA_INSTALL_DIR`; `support/analyzeHeadless` lives under it).

## Binary verification

```
$ shasum -a 256 ~/Downloads/TemIM3x/CD/TEMIM.EXE
03d56a132ff7c987488c6d28cc6ba9c4a28b6f9d085c53a3c5a0bfdd14e49e35  ...CD/TEMIM.EXE
```

Matches the expected hash exactly. The patched copy at `~/Downloads/TemIM3x/TIMWIN/TEMIM.EXE`
was also hashed for comparison and is confirmed **different**
(`891c57fc6242a155b865def9c17a2b6d8940ca443d67ae993bd64b66b8cabbb3`) — it was **not** used for
anything below.

## Project creation and import (headless)

The brief's suggested `-analyze` flag **does not exist** in Ghidra 12.1.2's `analyzeHeadless`
(confirmed via `analyzeHeadless` with no arguments, which prints full usage). Passing
`-analyze` literally causes it to be swallowed as an extra `-import` path argument and fails
with `InvalidInputException: .../-analyze is not a valid directory or file`. Auto-analysis
runs by default after `-import` unless `-noanalysis` is explicitly passed, so no extra flag is
needed. The actual working command:

```bash
export JAVA_HOME=/opt/homebrew/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home
export GHIDRA_HOME=/opt/homebrew/Cellar/ghidra/12.1.2/libexec
$GHIDRA_HOME/support/analyzeHeadless \
  /Users/u1470235/gits/OpenTIM/game-data/ghidra-project OpenTIM \
  -import /Users/u1470235/Downloads/TemIM3x/CD/TEMIM.EXE
```

This created project `OpenTIM` at `game-data/ghidra-project/OpenTIM` (git-ignored), imported
`TEMIM.EXE`, and ran full default auto-analysis (8s, all default analyzers: ASCII Strings,
Disassemble, Stack, Decompiler Switch Analysis, Segmented X86 Calling Conventions, x86
Constant Reference Analyzer, etc. — see the analyzer log for the full list). Ghidra correctly
identified:

- **Loader:** New Executable (NE)
- **Language/Compiler:** `x86:LE:16:Protected Mode:default`

During import, before analysis, Ghidra's NE loader searched for the imported libraries
`GDI`, `USER`, `KERNEL` (not found as real modules, as expected — this project doesn't ship
Windows 3.1 system DLLs), and instead applied its own bundled **Win16 export databases**:

```
Applying .../Ghidra/Features/Base/data/symbols/win16/kernel.exports
Applying .../Ghidra/Features/Base/data/symbols/win16/user.exports
Applying .../Ghidra/Features/Base/data/symbols/win16/gdi.exports
```

This turned out to be significant for Step 4 below.

The resulting memory map has exactly the segment layout implied by the brief's "34 segments":
27 `Code`/`Data` segments (`Code1`..`Code27`, `Data28`..`Data34`) plus 65 resource segments
(`Rsrc0`..`Rsrc64`) and one synthetic `EXTERNAL` block for unresolved imports.

## Step 4: Win16 import naming

### The repo's script does not run as-is

Following the brief, script directories were added and the script was invoked headless:

```bash
$GHIDRA_HOME/support/analyzeHeadless \
  game-data/ghidra-project OpenTIM \
  -process TEMIM.EXE -noanalysis \
  -scriptPath "reverse-engineering/ghidra-scripts;scripts" \
  -postScript rename-win16-fns.py
```

This fails with:

```
ERROR REPORT SCRIPT ERROR: rename-win16-fns.py : Ghidra was not started with PyGhidra. Python is not available
ghidra.app.script.GhidraScriptLoadException: Ghidra was not started with PyGhidra. Python is not available
	at ghidra.pyghidra.PyGhidraScriptProvider.getScriptInstance(PyGhidraScriptProvider.java:75)
	at ghidra.app.util.headless.HeadlessAnalyzer.runScriptsList(HeadlessAnalyzer.java:927)
	...
```

Root cause: Ghidra 12.1.2 has **completely removed the Jython script provider**. The install
contains only `licenses/Jython_License.txt` and doc stub files — no Jython runtime jar at
all. `.py` GhidraScripts are now exclusively handled by `PyGhidraScriptProvider`, which
requires the Ghidra process to have been launched through **PyGhidra** (a native-CPython
bridge via JPype/`pip install pyghidra`), which plain `analyzeHeadless` does not do. This
matches exactly what the brief anticipated: the script needs porting to PyGhidra.

### What was already solved without porting anything

Before attempting the port, it's worth noting that **Ghidra 12.1.2 already names all Win16
imports out of the box**, because of the `kernel.exports`/`user.exports`/`gdi.exports`
application seen during import above. Inspecting the program's imported thunks after import
(via a throwaway script, see below) showed:

```
TOTAL_EXTERNAL_THUNKS = 138   (KERNEL: 37, USER: 61, GDI: 40)
STILL_ORDINAL = 0
```

Zero functions were left as `Ordinal_NNN` — every import already has a real Win16 API name
(e.g. `GETVERSION`, `LOCALALLOC`, `GETTICKCOUNT`). So the *readability* goal of Step 4 (no
more bare ordinal numbers at call sites) is met without running any script at all. What is
**not** provided by Ghidra's built-in tables is proper parameter typing / calling convention —
every one of these thunks came back with `params=[]`, `calling convention="unknown"` — which
is exactly what `rename-win16-fns.py` (via the `win16fns/*.py` tables, themselves generated
from `scripts/*.spec`) is designed to add.

### Porting the script

To confirm the script genuinely can be ported (not just that it's unnecessary), it was
ported to native CPython and run against the existing project using `pyghidra` (installed via
`pip install pyghidra` — version 3.1.0 — into a throwaway venv at
`game-data/pyghidra-venv/`, not committed):

```python
import pyghidra
pyghidra.start(install_dir="/opt/homebrew/Cellar/ghidra/12.1.2/libexec")

with pyghidra.open_program(
        binary_path=None,
        project_location="game-data/ghidra-project", project_name="OpenTIM",
        program_name="TEMIM.EXE", analyze=False, nested_project_location=False
    ) as flat_api:
    program = flat_api.getCurrentProgram()
    ...
```

Findings from the port:

- The `win16fns/gdi.py`, `win16fns/kernel.py`, `win16fns/user.py` data tables under
  `reverse-engineering/ghidra-scripts/win16fns/` are **pure Python data literals** with no
  Jython-specific syntax — they import and work completely unmodified under CPython.
- The original script matches imports by ordinal (`Ordinal_NNN` thunk name). Since Ghidra's
  built-in export tables had already renamed everything, matching had to be done by
  **function name** (case-insensitively) against the same tables instead.
- `f.setName(...)`, `f.setCallingConvention(...)`, `f.setReturnType(...)`,
  `f.replaceParameters(...)` all need to run inside an explicit
  `program.startTransaction(...)` / `program.endTransaction(id, success)` pair —
  `pyghidra.open_program()`'s context manager does not itself open one for direct `Function`
  API mutations (only `FlatProgramAPI` convenience methods self-wrap).
- `Function.replaceParameters(list, FunctionUpdateType, bool, SourceType)` raised a JPype
  `TypeError: No matching overloads found` when passed a plain Python list, because the
  method is overloaded with a `Variable[]` varargs form and JPype can't disambiguate; wrapping
  the parameter list in `java.util.ArrayList()` resolved it.
- Everything else ports mechanically 1:1 (`USER_DEFINED` → `SourceType.USER_DEFINED`, etc.).

Running the ported script applied full `__stdcall16far` calling convention + typed
parameters + return types to **119 of 138** imported functions. The remaining 18 are simply
not present in the `.spec`-derived `win16fns` tables (e.g. `__AHSHIFT`, `GETWINFLAGS`,
`GETFREESPACE`) or are duplicate references to an already-renamed function encountered later
in iteration order — a coverage gap in the reference tables themselves, not a scripting
failure.

**Bottom line for Step 4:** import *names* are already present with zero extra work on
current Ghidra; import *type signatures* require the script, and the script does port to
PyGhidra with the three small changes above. Nothing was committed to
`reverse-engineering/ghidra-scripts/` — the ported version lives only in the throwaway,
git-ignored `game-data/scratch-scripts/` and is not part of this repository. If this project
wants the ported script to be a first-class supported tool, that porting work (updating
`rename-win16-fns.py` itself, plus a documented `pip install pyghidra` step) is a reasonable
follow-up task, out of scope here.

## Segment-to-address correspondence

The 16-bit NE loader in Ghidra assigns each of the file's segments a synthetic protected-mode
selector as the segment component of its address, and — critically — **this happens to be
exactly the same numbering convention used by the `TIMWIN: SSSS:OOOO` comments** in the C/Rust
source:

```
segment_selector(N) = 0x1000 + (N - 1) * 8      (N = 1-based index into the NE segment table)
```

Confirmed directly from the program's memory map (`getMemory().getBlocks()`):

```
Code1  start=1000:0000 end=1000:2818
Code2  start=1008:0000 end=1008:1f61
Code3  start=1010:0000 end=1010:13c3
...
Code21 start=10a0:0000 end=10a0:0dc3
Code22 start=10a8:0000 end=10a8:5113   <-- segment "10a8"
Code23 start=10b0:0000 end=10b0:0bfe
...
Code27 start=10d0:0000 end=10d0:1073
Data28 start=10d8:0000 ...
...
Data34 start=1108:0000 ...
Rsrc0..Rsrc64 (resource segments, not part of the 34 code/data segments)
EXTERNAL start=1318:0000 ...
```

**Segment `10a8` is `Code22`, the 22nd segment in the file.** Since 16-bit segment:offset
addressing keeps the offset fixed regardless of which selector a segment is loaded at, a
`TIMWIN: SSSS:OOOO` comment can be typed **directly and unchanged** as a Ghidra address
(`goTo("SSSS:OOOO")` in the GUI, or `addressFactory.getAddress("SSSS:OOOO")` in scripts) — no
segment-index translation is required at all. This is a much simpler correspondence than the
34-segments caveat in the brief implied might be needed; it held cleanly for this binary.
(The likely explanation: TIMWIN's own DOSBox-X captured runtime selectors evidently followed
this same "base 0x1000, step 8, in segment-table order" allocation that standard-mode Windows
3.1 uses for a module's own segments, and Ghidra's NE loader synthesizes the same scheme.)

## Step 5: Validation — `remove_part_from_linked_list` at `10a8:1e18`

`src/tim_c.rs:132-150` documents:

```rust
/// TIMWIN: 10a8:1e18
#[no_mangle]
pub extern "C" fn remove_part_from_linked_list(part: *mut Part) {
    unsafe {
        (*(*part).prev).next = (*part).next;
        if !(*part).next.is_null() {
            (*(*part).next).prev = (*part).prev;
        }
    }
}
```

Resolving address `10a8:1e18` in the Ghidra program:

```
Resolved address: 10a8:1e18
Function containing address: FUN_10a8_1e18
Function entry:   10a8:1e18   (exact match, not merely "contained in")
Function body size: 46 bytes
```

Disassembly:

```
10a8:1e18 MOV AX,DS
10a8:1e1a NOP
10a8:1e1b INC BP
10a8:1e1c PUSH BP
10a8:1e1d MOV BP,SP
10a8:1e1f PUSH DS
10a8:1e20 MOV DS,AX
10a8:1e22 XOR AX,AX
10a8:1e24 CALLF 0x1000:228f      ; runtime helper (far function entry / stack check thunk)
10a8:1e29 PUSH SI
10a8:1e2a MOV SI,word ptr [BP + 0x6]     ; SI = part
10a8:1e2d MOV AX,word ptr [SI]          ; AX = part->next   (field 0)
10a8:1e2f MOV BX,word ptr [SI + 0x2]    ; BX = part->prev   (field 1)
10a8:1e32 MOV word ptr [BX],AX          ; prev->next = next   <-- unconditional, no null check on prev
10a8:1e34 CMP word ptr [SI],0x0         ; part->next == 0 ?
10a8:1e37 JZ 0x10a8:1e41
10a8:1e39 MOV AX,word ptr [SI + 0x2]    ; AX = part->prev
10a8:1e3c MOV BX,word ptr [SI]          ; BX = part->next
10a8:1e3e MOV word ptr [BX + 0x2],AX    ; next->prev = prev   <-- only if next != null
10a8:1e41 POP SI
10a8:1e42 POP DS
10a8:1e43 POP BP
10a8:1e44 DEC BP
10a8:1e45 RETF
```

This is **exactly** the doubly-linked-list unlink described in the Rust port: the unconditional
`part->prev->next = part->next` (with no null check on `part->prev`, matching the "no null
check existed there either" note in the Rust safety comment), followed by the conditional
(`if (part->next) { ... }`) `part->next->prev = part->prev`. The struct field layout the
disassembly implies (`next` at offset 0, `prev` at offset 2) is internally self-consistent
across both halves of the function.

**Result: VALIDATION PASSES.** The `TIMWIN: SSSS:OOOO` → Ghidra address mapping is confirmed
correct with no translation needed beyond identifying which literal segment value the file's
Nth segment lands on (trivial to read off the memory map). This is not a blocking finding —
later work identifying `stub_XXXX_XXXX` functions from their TIMWIN address comments can
proceed by typing the address directly into Ghidra's "Go To" address bar.

## Task 7: identifying the last 4 leaf stubs

Ghidra decompilation of all four addresses below was run against the same validated
`OpenTIM` project (`pyghidra_dump_task7.py`, throwaway, not committed). All four disassemblies
matched the existing C 1:1 where the C already had real logic, and revealed real (non-stub)
logic at the two addresses whose C bodies had been reduced to deliberate no-ops.

| stub | renamed to | confidence | what was established | what remains unknown |
|---|---|---|---|---|
| `stub_1050_025e` | `set_bounce_side_flags` | High (mechanics); medium (deeper meaning) | Ghidra decompile at `1050:025e` matches the existing "Accurate" C exactly, instruction for instruction. Classifies which side of a `Line`'s x-range a query point falls on, setting one or both of a 2-byte output (`bounce_field_0x86`) accordingly. Only caller is `stub_1050_0550` (still unrenamed), part of the general border-bounce collision path used by any two colliding bordered parts. | Why the destination is specifically called `bounce_field_0x86`, i.e. what a "side" flag concretely controls later in bounce resolution, is still not established — the field itself was left unrenamed. |
| `stub_10a8_4509` | `llama2_insert_by_force` | High (mechanics, confirmed byte-for-byte against Ghidra); medium (why) | Maintains `LLAMA_2` as a list of parts to be (re-)simulated this frame, sorted descending by `force`, drawing free nodes from the `LLAMA_1` pool (`initialize_llamas`). Refuses to insert if `part_b` is already queued with an equal-or-higher force. Called from `teeter_totter_bounce` and the rope-to-teeter-totter force-propagation code — both "hand off my force to a connected part so it reacts next" scenarios. | The `Llama` naming itself remains a codename (pre-existing, not resolved by this task) — no evidence surfaced for what the original function/struct was actually called. |
| `stub_10a8_2bea` | `queue_dirty_rect` | High (mechanics, from Ghidra decompile) | NOT a no-op in the original binary (469 bytes of real logic): converts world-space `pos`/`size` (or two corner points) to screen space via global scroll offsets and inserts/dedupes the resulting rectangle into a global pending-redraw list. Never touches `Part`/simulation state — confirms the prior port's guess ("might be related to drawing") was correct. Left as the no-op the C body already was, since this project's renderer repaints every frame rather than tracking dirty rectangles. | The exact original name/purpose within the legacy GDI blitter (e.g. whether it fed a real `InvalidateRect`-style call) was not traced further, since it has no bearing on simulation correctness. |
| `stub_10a8_28f6` | `queue_rope_dirty_rects` | High (mechanics, from Ghidra decompile) | NOT a no-op in the original binary (631 bytes): walks a rope/pulley chain from `part->rope_data[0]`, using the same `links_to[rope_slot]` traversal as `calculate_rope_sag`, and calls `queue_dirty_rect` to cover each segment's previous-frame extent (endpoints from `RopeData::ends_pos_prev1`, expanded by sag) plus a 16x16 rect at each endpoint (anchor/pin sprite). Simulation-mode covers only the immediate neighbour(s); design-mode walks the whole chain. Confirms the prior port's guess ("Called when ropes are used... related to drawing") was correct. Left as the pre-existing no-op. | Same caveat as `queue_dirty_rect` — legacy rendering detail, not simulation-relevant. |

All four are exercised by the test gate's own 7 levels, not merely read about: `L20.LEV` and
`L24.LEV` both contain `Rope`/`Pulley` parts (`L20` also has 2 `TeeterTotter`s), so
`llama2_insert_by_force`, `queue_dirty_rect` and `queue_rope_dirty_rects` all run during
`./scripts/verify.sh`. `set_bounce_side_flags` sits on the general border-bounce collision
path and is exercised whenever any two bordered parts collide (e.g. the basketballs/inclines
in `L6.LEV`/`L31.LEV`/`L79.LEV`). Confirmed by temporarily adding a scratch example
(`examples/scratch_partcount.rs`, not committed) that iterates each level's loaded parts and
prints a `PartType` histogram.

## Procedure summary (for future identification work)

1. `brew install ghidra` (formula; provides `openjdk@21` too).
2. Set `JAVA_HOME=/opt/homebrew/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home`.
3. Confirm `shasum -a 256 ~/Downloads/TemIM3x/CD/TEMIM.EXE` equals
   `03d56a132ff7c987488c6d28cc6ba9c4a28b6f9d085c53a3c5a0bfdd14e49e35`.
4. `analyzeHeadless <project_dir> <project_name> -import <path/to/CD/TEMIM.EXE>` (analysis
   runs by default; do not pass a nonexistent `-analyze` flag).
5. Import naming for GDI/USER/KERNEL calls is automatic (Ghidra's bundled win16 `.exports`
   tables). For full parameter typing/calling convention, either open the project in the GUI
   and use `Window > Script Manager` with PyGhidra configured, or use the `pyghidra` Python
   package (`pip install pyghidra`) to drive `FlatProgramAPI` directly as demonstrated above.
6. To find a `stub_XXXX_XXXX` function's real behaviour from its `TIMWIN: SSSS:OOOO` comment:
   type `SSSS:OOOO` directly into Ghidra's Go To address bar (GUI) — no translation needed.
   In the GUI, segment `10a8` is inside Ghidra's synthesized `Code22` block; segment N in
   general is at base selector `0x1000 + (N-1)*8`.
