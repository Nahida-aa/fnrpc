---
"@fnrpc/client": patch
---

Fix UTF-8 corruption of non-ASCII (e.g. CJK) inputs sent over GET query strings.

The server-side `percent_decode` (and the `fnrpc-axum` `urlencoding_decode` copy) cast each decoded byte directly to a `char`, which mangled multi-byte UTF-8 sequences into mojibake. Decoded bytes are now collected into a `Vec<u8>` and decoded as a single UTF-8 sequence via `from_utf8_lossy`.

The `fnrpc-axum` e2e now covers `zh_input` (GET) and `zh_input_post` (POST) to assert end-to-end UTF-8 transparency.
