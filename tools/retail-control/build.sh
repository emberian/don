#!/bin/sh
set -eu

cd "$(dirname "$0")"
zig cc -target x86-windows-gnu -O2 -Wall -Wextra -Werror -shared \
  -o retail_control.dll retail_control.c retail_control.def
file retail_control.dll
