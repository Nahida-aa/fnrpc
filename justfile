# fnrpc root justfile — delegates to benches/justfile
bench framework concurrency="200" duration="3" *filter:
    @just --justfile benches/justfile bench {{framework}} {{concurrency}} {{duration}} {{filter}}

bench-all concurrency="200" duration="3" *filter:
    @just --justfile benches/justfile bench-all {{concurrency}} {{duration}} {{filter}}
