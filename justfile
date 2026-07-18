# fnrpc root justfile — delegates to benches/justfile
bench framework concurrency="200" duration="3" *filter:
    @just --justfile benches/justfile bench {{framework}} {{concurrency}} {{duration}} {{filter}}
