# Error Codes

All ZamSync HTTP errors return a JSON body with two fields:

```json
{
  "error": "SCHEMA_VIOLATION",
  "message": "missing required field: patient_id"
}
```

| Field | Type | Description |
|-------|------|-------------|
| `error` | string | Stable machine-readable code -- safe to `switch` on |
| `message` | string | Human-readable detail -- may change between versions |

Error codes are **additive and stable**: existing codes are never renamed or removed. New codes may be added in future versions.

---

## Code reference

| Code | HTTP | When |
|------|------|------|
| [`INVALID_JSON`](#invalid_json) | 400 | Request body is not valid JSON or `Content-Type` is missing |
| [`SCHEMA_VIOLATION`](#schema_violation) | 422 | Payload fails the `--schema` validation rule |
| [`WAL_UNAVAILABLE`](#wal_unavailable) | 503 | WAL file unavailable (disk full, corrupt, permission error) |
| [`INTERNAL_ERROR`](#internal_error) | 500 | Unexpected internal error |

---

## INVALID_JSON

**HTTP 400 Bad Request**

The request body could not be parsed as JSON, or the `Content-Type` header is not `application/json`.

```bash
# Trigger: malformed body
curl -X POST http://localhost:8080/submit \
  -H 'Content-Type: application/json' \
  -d 'not json'
```

```json
{
  "error": "INVALID_JSON",
  "message": "Failed to parse the request body as JSON: ..."
}
```

**How to handle**

Fix the request body. Ensure:
- `Content-Type: application/json` header is present
- Body is valid JSON
- Body matches the expected shape: `{"payload": <any JSON>}`

---

## SCHEMA_VIOLATION

**HTTP 422 Unprocessable Entity**

The payload is valid JSON but fails the validation rule configured with `--schema` on the server.

This error only occurs when the hub is started with `--schema json-required:<field1>,<field2>,...`. If no schema is configured, all payloads are accepted.

```bash
# Hub started with: --schema json-required:patient_id,type
curl -X POST http://localhost:8080/submit \
  -H 'Content-Type: application/json' \
  -d '{"payload": {"name": "John"}}'
```

```json
{
  "error": "SCHEMA_VIOLATION",
  "message": "missing required field: patient_id"
}
```

**How to handle**

Ensure the payload includes all required fields. Check the hub configuration for the `--schema` flag to know which fields are mandatory.

```bash
# Correct: includes all required fields
curl -X POST http://localhost:8080/submit \
  -H 'Content-Type: application/json' \
  -d '{"payload": {"patient_id": "P-001", "type": "admission"}}'
```

---

## WAL_UNAVAILABLE

**HTTP 503 Service Unavailable**

The WAL file cannot be read or written. This is a server-side problem -- the client request was valid.

Common causes:

- Disk full
- Data directory deleted or moved
- Filesystem mounted read-only
- WAL file corrupted (use `zamsync info` to inspect)
- Permission denied on the data directory

```json
{
  "error": "WAL_UNAVAILABLE",
  "message": "IO error: No space left on device (os error 28)"
}
```

**How to handle**

Retry with exponential backoff. The server will recover automatically once the underlying problem is resolved (disk freed, filesystem remounted, etc.). Do not retry immediately in a tight loop.

```js
// Retry pattern
async function submitWithRetry(payload, maxRetries = 5) {
  let delay = 1000;
  for (let i = 0; i < maxRetries; i++) {
    const r = await fetch('/submit', { method: 'POST', body: JSON.stringify({payload}) });
    if (r.status !== 503) return r;
    await new Promise(ok => setTimeout(ok, delay));
    delay = Math.min(delay * 2, 30_000);
  }
}
```

---

## INTERNAL_ERROR

**HTTP 500 Internal Server Error**

An unexpected error occurred inside ZamSync. This indicates a bug.

```json
{
  "error": "INTERNAL_ERROR",
  "message": "..."
}
```

**How to handle**

Please open an issue at [github.com/Etoile-Bleu/ZamSync/issues](https://github.com/Etoile-Bleu/ZamSync/issues) with the full `message` value and the ZamSync version (`zamsync --version`).
