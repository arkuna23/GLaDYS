# gladys-web API

Bind default `0.0.0.0:3924`. Token: `GLADYS_WEB_TOKEN` (`web.toml` `token_env`).
Web calls Memory with `GLADYS_MEMORY_TOKEN`; clients never send the Memory token.

## Auth

`POST /api/login` (no Bearer):

```bash
curl -sS http://127.0.0.1:3924/api/login \
  -H 'Content-Type: application/json' \
  -d "{\"token\":\"$GLADYS_WEB_TOKEN\"}"
```

`{"ok":true}` or `401`.

All `/api/memories` and `/api/scopes` routes: `Authorization: Bearer <GLADYS_WEB_TOKEN>`.

`GET /health` is public and returns `ok`.

## Mapping

| Web | Memory |
| --- | --- |
| `/api/memories` | `/v1/memories` |
| `/api/memories/{id}` | `/v1/memories/{id}` |
| `/api/scopes` | `/v1/scopes` |

Method, query string, and JSON body are forwarded. Status and body come back as-is.

## Memories

`GET /api/memories?layer=&channel=&kind=&peer=&person=&q=&limit=&before_ts=`

- `q` empty: SQL list (newest first)
- `q` set: FTS search
- `limit` default 50, max 200
- `before_ts`: older than this unix seconds (list pagination)

```bash
curl -sS -H "Authorization: Bearer $GLADYS_WEB_TOKEN" \
  'http://127.0.0.1:3924/api/memories?layer=global'
```

`GET /api/memories/{id}`

`POST /api/memories`

```json
{"layer":"global","text":"be brief"}
```

```json
{"layer":"conversation","text":"group rule","channel":"onebot","conversation":{"kind":"group","peer":"1"}}
```

```json
{"layer":"person","text":"alice likes short answers","channel":"onebot","person":"10001"}
```

`PATCH /api/memories/{id}` `{"text":"..."}` — id / layer / scope unchanged.

`DELETE /api/memories/{id}` → `{"ok":true}`

## Scopes

`GET /api/scopes` — distinct channels, conversations `{channel,kind,peer}`, persons `{channel,person}` for filters.
