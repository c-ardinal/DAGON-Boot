# Test vectors (third-party data, unmodified)

The tests in `tests/` read these files; nothing here is built into a
program.

| File | Source | SHA-256 |
|---|---|---|
| `ed25519_test.json` | Project Wycheproof, `testvectors_v1/ed25519_test.json` at commit `65ca494727a52944c204441c860b039e6786b99b` of `https://github.com/C2SP/wycheproof`, copied unchanged (172,628 bytes, 210 test cases) | `ea80ffebac085e21cb9d3e903e7fa2b448d96702da5b31f061d0b83b430b74b3` |
| `LICENSE-WYCHEPROOF` | `LICENSE` of the same Wycheproof commit (Apache License 2.0), which covers `ed25519_test.json` | `58d1e17ffe5109a7ae296caafcadfdbe6a7d176f0bc4ab01e12a689b0499d8bd` |
| `rfc8032.txt` | RFC 8032 section 7.1, the five Ed25519 test vectors (TEST 1, TEST 2, TEST 3, TEST 1024, TEST SHA(abc)), one per line: name, secret key, public key, signature, message (`-` for the empty message), all hex; the values are copied from `https://www.rfc-editor.org/rfc/rfc8032.txt` with only the line breaks of the RFC removed | `590728244b421636fe117eba9ddf1aa47b104d8c411b805d5fda56fea7668629` |

`tests/vectors.rs` checks the size, the case count and the SHA-256 of
`ed25519_test.json` before it reads any case, and the SHA-256 of
`rfc8032.txt`.
