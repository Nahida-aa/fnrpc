# fnrpc root justfile — delegates to benches/justfile
bench *args:
    @just --justfile benches/justfile --working-directory . {{args}}
