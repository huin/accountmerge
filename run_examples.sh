#!/bin/sh

set -e

mkdir -p example_output

for i in 1 2; do
    cargo run -q -- import nationwide-csv examples/statement${i}.csv \
        > example_output/statement${i}-raw.journal
    cargo run -q -- apply-rules -r examples/rules.ron \
        example_output/statement${i}-raw.journal \
        > example_output/statements${i}-ruled.journal
done

cargo run -q -- merge \
    examples/initial.journal \
    example_output/statements*-ruled.journal \
    > example_output/merged.journal
