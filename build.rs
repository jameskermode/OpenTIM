use std::path::PathBuf;

fn main() {
    let target = std::env::var("TARGET").unwrap_or_default();
    let c_sources = &["foo.c", "globals.c", "main.c", "part_defs.c", "draw_rope.c"];

    let mut builder = cc::Build::new();

    for filename in c_sources {
        builder.file(format!("c_src/{}", filename));
    }

    if target.starts_with("wasm32") {
        // Apple clang ships no WebAssembly backend, so cross-compile the C half with `zig cc`.
        // The cc crate wants a single executable, hence the wrapper.
        let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
        let wrapper = out_dir.join("zig-cc");
        std::fs::write(&wrapper, "#!/bin/sh\nexec zig cc \"$@\"\n").expect("write zig-cc wrapper");

        // The host `ar` writes a Mach-O style archive that wasm-ld cannot read: it skips the
        // members silently and every C symbol degrades into an unresolved wasm import, which
        // still links successfully. Use zig's llvm-ar instead.
        let ar_wrapper = out_dir.join("zig-ar");
        std::fs::write(&ar_wrapper, "#!/bin/sh\nexec zig ar \"$@\"\n").expect("write zig-ar wrapper");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for w in [&wrapper, &ar_wrapper] {
                std::fs::set_permissions(w, std::fs::Permissions::from_mode(0o755))
                    .expect("chmod wrapper");
            }
        }

        builder.compiler(&wrapper);
        builder.archiver(&ar_wrapper);
        // The cc crate only emits --target for compilers it recognises, and it does not
        // recognise the wrapper, so pass zig's triple explicitly. Without this zig cc
        // silently builds for the host and the C ends up as unresolved wasm imports.
        builder.flag("--target=wasm32-freestanding");
        builder.flag("-ffreestanding");
        // zig cc turns UBSan on by default in debug builds, which leaves __ubsan_handle_*
        // as unresolved wasm imports. The decompiled core deliberately relies on wrapping
        // arithmetic, so the instrumentation is unwanted anyway.
        builder.flag("-fno-sanitize=undefined");

        // Freestanding wasm has no libc headers. The implementations live in
        // src/wasm_libc.rs (and compiler_builtins for mem*), so all the C needs is
        // declarations. stddef.h/stdint.h come from the compiler itself.
        let inc = out_dir.join("libc-shim");
        std::fs::create_dir_all(&inc).expect("create libc-shim dir");
        std::fs::write(
            inc.join("stdlib.h"),
            "#pragma once\n\
             #include <stddef.h>\n\
             void *malloc(size_t);\n\
             void *calloc(size_t, size_t);\n\
             void free(void *);\n\
             int abs(int);\n",
        )
        .expect("write stdlib.h shim");
        std::fs::write(
            inc.join("string.h"),
            "#pragma once\n\
             #include <stddef.h>\n\
             void *memcpy(void *, const void *, size_t);\n\
             void *memmove(void *, const void *, size_t);\n\
             void *memset(void *, int, size_t);\n\
             int memcmp(const void *, const void *, size_t);\n",
        )
        .expect("write string.h shim");
        std::fs::write(
            inc.join("math.h"),
            "#pragma once\n\
             float sqrtf(float);\n",
        )
        .expect("write math.h shim");

        builder.flag(&format!("-isystem{}", inc.display()));
    }

    builder.compile("tim_c");

    // ---- Differential-testing reference C (tests only, native only) ----
    //
    // tests/differential/reference.c holds `ref_part_borders_intersect`, a frozen,
    // verbatim-from-git-history copy of the original decompiled `part_borders_intersect`
    // (see the header comment in that file for provenance). It is compiled into its own
    // separate static lib -- distinct from "tim_c" above -- so linking never even
    // pretends it's part of the game's real C sources.
    //
    // build.rs has no reliable way to ask cargo "is this specifically a `cargo test`
    // invocation?" -- build scripts are re-used across `build`/`test`/`check` for the same
    // (package, target, profile, features) tuple, and their env vars are identical either
    // way (verified empirically: diffing every CARGO_*/PROFILE/TARGET var between `cargo
    // build --lib -vv` and `cargo test --lib --no-run -vv` on this crate showed zero
    // difference). So this always compiles the tiny reference.c on non-wasm targets.
    // That's fine in practice: nothing in the normal binary or cdylib ever references
    // `ref_part_borders_intersect`, so the linker never pulls this object out of the
    // archive for anything except the differential test binary (which does reference it).
    // The normal build's output is therefore unaffected -- same symbols, same behaviour --
    // only the differential test binary actually links this code in.
    //
    // wasm is excluded outright, per the requirement that this must never affect (or even
    // attempt to affect) the wasm build: the freestanding wasm target has no libc, no
    // `struct Part`-compatible ABI expectations checked here, and no reason to carry test
    // infrastructure into a build that ships to players.
    if !target.starts_with("wasm32") {
        cc::Build::new()
            .file("tests/differential/reference.c")
            .include("c_src")
            .compile("tim_c_differential");
    }
}
