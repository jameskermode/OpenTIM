#!/bin/sh
# Run the original TEMIM (Windows 3.x build) under DOSBox-X, as a behavioural oracle.
#
# This is the exact binary the decompiled C was reverse-engineered from: every
# /* TIMWIN: 10a8:1e46 */ comment is a segment:offset in CD/TEMIM.EXE, whose hashes are
# pinned in reverse-engineering/README.md.
#
# Requires: brew install dosbox-x   (the formula, not the dosbox-x-app cask, which is
#           deprecated for failing the macOS Gatekeeper check)
#
# Expects a Windows 3.1 image containing the installed game at game-data/temim-win,
# with TIMWIN/TEMIM.EXE and a WINDOWS/ directory.
#
# Two things that are easy to get wrong:
#
#  * 386 enhanced mode (`win /3`) does not start from a mounted host folder -- it reaches
#    "[Enhanced mode]" and dies before writing a boot log, because of its direct disk
#    access. Standard mode (`win /s`) works. For enhanced mode, build a real hard disk
#    image with DOSBox-X's imgmake instead of mounting a directory.
#
#  * WIN.COM lives in C:\WINDOWS, so run it from there and give TEMIM an absolute path.
#    Doing `cd \TIMWIN` first just fails with "bad command or file name" and leaves you
#    at a DOS prompt, which looks like no graphical output at all.
set -e

cd "$(dirname "$0")/.."
IMAGE="$PWD/game-data/temim-win"

if [ ! -f "$IMAGE/TIMWIN/TEMIM.EXE" ]; then
    echo "No Windows TEMIM image at $IMAGE" >&2
    echo "It needs TIMWIN/TEMIM.EXE and a WINDOWS/ directory." >&2
    exit 1
fi

CONF="$(mktemp -t temim).conf"
cat > "$CONF" <<EOF
[sdl]
fullscreen=false
autolock=false
[dosbox]
machine=svga_s3
memsize=16
captures=$PWD/game-data/dosbox-captures
[cpu]
core=normal
cycles=20000
[autoexec]
mount c $IMAGE
c:
cd \\WINDOWS
win /s c:\\TIMWIN\\TEMIM.EXE
EOF

mkdir -p game-data/dosbox-captures
echo "Starting TEMIM under Windows 3.1 standard mode."
echo "Screenshots: Ctrl-F5 -> game-data/dosbox-captures"
exec dosbox-x -conf "$CONF"
