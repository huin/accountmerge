#!/bin/sh

set -e

mkdir -p example_output

cargo run -q -- generate-fingerprints - \
    < examples/initial.journal \
    > example_output/initial.journal

for i in 1 2; do
    cargo run -q \
        -- import --output example_output/statement${i}-raw.journal \
        nationwide-csv examples/statement${i}.csv
    cargo run -q -- apply-rules -r examples/rules.ron \
        --output example_output/statements${i}-ruled.journal \
        example_output/statement${i}-raw.journal
done

cargo run -q -- merge \
    --output example_output/merged.journal \
    example_output/initial.journal \
    example_output/statements*-ruled.journal

# Should be able to re-merge the merged result above with all its inputs and
# get the same result.
cargo run -q -- merge \
    --output example_output/merged2.journal \
    example_output/merged.journal \
    example_output/initial.journal \
    example_output/statements*-ruled.journal
