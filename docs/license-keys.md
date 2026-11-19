# Licence keys

pdfr licence keys are made by pdfr.app and checked by pdfr **offline**. This is
the contract between the two. The Rust side is `crates/license`. The
TypeScript side is `src/lib/licenses/sign.ts` in pdfr.app.

## Format

```
PDFR1.<payload>.<signature>
```

- `payload`: a UTF-8 JSON object, base64url-encoded without padding.
- `signature`: the Ed25519 signature (RFC 8032, 64 bytes) over the **payload's
  bytes as encoded**, base64url-encoded without padding (always 86 characters).

The signature covers the exact bytes, so the JSON doesn't need canonical form.
A verifier decodes the payload, reads `kid`, picks that public key, verifies
the signature, and only then trusts the rest.

| Field | Type | Meaning |
|---|---|---|
| `v` | number | Format version: `1` |
| `kid` | number 1–254 | Which signing key made it (255 is the test key) |
| `id` | string | The licence's UUID in pdfr.app |
| `plan` | string | `personal`, `business`, `founder` (pdfr accepts unknown plans, which allows future plans) |
| `name` | string | Who it is registered to, shown as "Registered to …" |
| `issued` | string | `YYYY-MM-DD` |

The key never contains an email address. Revocation and the device limit are
online-only (see below).

## Keys

- The private key is a 32-byte Ed25519 seed. pdfr.app reads it from the
  `LICENSE_SIGNING_KEY` environment variable (base64url) and its id from
  `LICENSE_KID`. It is never committed. Make one with
  `cargo run -p pdfr-license -- keygen --kid <n> --out <file>`.
- The public keys are in `TRUSTED_KEYS` in `crates/license/src/lib.rs`.
  To rotate: generate kid n+1, add its public key to pdfr, release, then
  switch pdfr.app. Keep old public keys, so issued keys keep working.
- In Node, sign with the built-in `crypto` module:

```ts
import { createPrivateKey, sign } from "node:crypto";
const key = createPrivateKey({ key: { kty: "OKP", crv: "Ed25519", d: seedB64url, x: publicB64url }, format: "jwk" });
const payload = Buffer.from(JSON.stringify({ v: 1, kid, id, plan, name, issued }));
const licenceKey = `PDFR1.${payload.toString("base64url")}.${sign(null, payload, key).toString("base64url")}`;
```

## Test vector

Every implementation must reproduce this exactly.

- seed (hex): `000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f`
- seed (base64url): `AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8`
- public key (base64url): `A6EHv_POEL4dcN0Y50vAmWfk1jCbpQ1fHdyGZBJVMbg`
- payload JSON (exactly these bytes):
  `{"v":1,"kid":255,"id":"00000000-0000-4000-8000-000000000001","plan":"business","name":"Test Licensee","issued":"2026-11-17"}`
- key:

```
PDFR1.eyJ2IjoxLCJraWQiOjI1NSwiaWQiOiIwMDAwMDAwMC0wMDAwLTQwMDAtODAwMC0wMDAwMDAwMDAwMDEiLCJwbGFuIjoiYnVzaW5lc3MiLCJuYW1lIjoiVGVzdCBMaWNlbnNlZSIsImlzc3VlZCI6IjIwMjYtMTEtMTcifQ.1RTd1fKstl83t5f5eTYD6C_OjAz85DUgOnr5T-pWLF1RESIIRLMHeQXWRQN-XErTy2VFvDOA-cpGYDfmYsCLBg
```

Release builds never trust kid 255. Debug builds also trust
`PDFR_LICENSE_TEST_KEY=<kid>:<base64url public key>`, for testing against a
local or staging pdfr.app.

## Online check

When a registered copy has internet, it checks its licence at launch (if the
last check is over an hour old) and once a day while it runs:

```
POST https://pdfr.app/api/v1/licenses/check
{"id":"<licence uuid>","install":"<32 hex chars>","version":"0.2.0","os":"linux"}
```

`install` is a random id made on first run (`<data dir>/install_id`). pdfr.app
stores only its SHA-256 hash. Nothing else is sent: no key, no name, no files.

The answer is `{"status": "...", "message": "...", "manage_url": "https://pdfr.app/account"}`.
`message` and `manage_url` are optional.

| status | pdfr does |
|---|---|
| `active` | nothing |
| `unknown` | nothing (valid signature, but no record of the licence, for example one issued by hand) |
| `device_limit` | stays registered, and asks the user to release a computer at pdfr.app/account |
| `revoked` | removes the key (refund, chargeback, leaked key) and shows `message` |

Network errors, timeouts and any other answer change nothing. A copy
registered offline stays registered.
