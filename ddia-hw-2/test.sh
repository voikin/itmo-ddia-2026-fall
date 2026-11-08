#!/bin/sh
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
exec cargo test --locked --manifest-path "$root/Cargo.toml" "$@"
